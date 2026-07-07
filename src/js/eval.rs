//! Tree-walking evaluator for the built-in JavaScript subset.
//!
//! Evaluates the AST produced by [`super::parser`]. Provides a pragmatic
//! runtime: variables with lexical scoping, closures, control flow, and a
//! small standard library (`document.write`, strings, arrays, `Math`,
//! `JSON`, `console`, global constructors). The goal is to execute simple
//! inline scripts (notably `document.write(...)`) so JS-rendered content can
//! be captured. It is deliberately best-effort: scripts using unsupported
//! features fail fast and are skipped by the caller.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::ast::*;
use super::parser::parse;

/// Maximum number of AST nodes evaluated before the interpreter bails out.
/// Guards against malicious or buggy infinite loops in fetched pages.
const MAX_STEPS: u64 = 2_000_000;

pub type NativeFn = Rc<dyn Fn(&[Value]) -> Result<Value, String>>;

#[derive(Clone)]
pub(crate) enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<ObjectData>>),
    Func(Rc<Closure>),
    Native(NativeFn),
}

pub(crate) struct ObjectData {
    pub props: HashMap<String, Value>,
}

impl ObjectData {
    fn new() -> Self {
        Self {
            props: HashMap::new(),
        }
    }
}

pub(crate) struct Closure {
    pub name: Option<String>,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub env: ScopeRef,
}

pub(crate) type ScopeRef = Rc<RefCell<Scope>>;

pub(crate) struct Scope {
    vars: HashMap<String, Value>,
    parent: Option<ScopeRef>,
}

impl Scope {
    fn child(parent: &ScopeRef) -> ScopeRef {
        Rc::new(RefCell::new(Scope {
            vars: HashMap::new(),
            parent: Some(parent.clone()),
        }))
    }

    fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.vars.get(name) {
            return Some(v.clone());
        }
        self.parent.as_ref().and_then(|p| p.borrow().get(name))
    }

    /// Assign to an existing binding in the nearest defining scope; if it does
    /// not exist yet, create it in the current scope (sloppy mode fallback).
    fn assign(&mut self, name: &str, value: Value) {
        if self.vars.contains_key(name) {
            self.vars.insert(name.to_string(), value);
            return;
        }
        if let Some(p) = &self.parent {
            let mut parent = p.borrow_mut();
            if parent.has(name) {
                parent.assign(name, value);
                return;
            }
        }
        self.vars.insert(name.to_string(), value);
    }

    fn has(&self, name: &str) -> bool {
        self.vars.contains_key(name)
            || self.parent.as_ref().map_or(false, |p| p.borrow().has(name))
    }
}

enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

struct TimerEntry {
    fire_at_ms: u64,
    interval_ms: Option<u64>,
    callback: Value,
}

/// A self-contained JS interpreter instance with its own global scope and
/// `document` write buffer.
pub struct Interpreter {
    global: ScopeRef,
    doc_buf: Rc<RefCell<String>>,
    pending_timers: Rc<RefCell<Vec<TimerEntry>>>,
    steps: u64,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let global = Rc::new(RefCell::new(Scope {
            vars: HashMap::new(),
            parent: None,
        }));
        let doc_buf = Rc::new(RefCell::new(String::new()));
        let pending_timers = Rc::new(RefCell::new(Vec::new()));
        let me = Self {
            global: global.clone(),
            doc_buf: doc_buf.clone(),
            pending_timers: pending_timers.clone(),
            steps: 0,
        };
        me.install_globals(&global, &doc_buf, &pending_timers);
        me
    }

    /// Parse and execute a script source string in the shared global scope.
    pub fn run_script(&mut self, src: &str) -> Result<(), String> {
        let stmts = parse(src).map_err(|e| e.to_string())?;
        let env = self.global.clone();
        self.exec_block(&stmts, &env)?;
        Ok(())
    }

    /// Run scheduled callbacks (`setTimeout`, `setInterval`, `requestAnimationFrame`)
    /// whose fire time is within `max_delay_ms`.
    pub fn flush_timers(&mut self, max_delay_ms: u64) {
        const MAX_TIMER_FIRES: u32 = 1_000;
        let mut fires = 0u32;
        loop {
            if fires >= MAX_TIMER_FIRES {
                break;
            }
            let mut timers = self.pending_timers.borrow_mut();
            let idx = timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.fire_at_ms <= max_delay_ms)
                .min_by_key(|(_, t)| t.fire_at_ms)
                .map(|(i, _)| i);
            let Some(idx) = idx else { break };
            let entry = timers.remove(idx);
            drop(timers);

            let callback = entry.callback.clone();
            let _ = self.call_value(entry.callback, &[]);
            fires += 1;

            if let Some(interval) = entry.interval_ms {
                let next = entry.fire_at_ms.saturating_add(interval);
                if next <= max_delay_ms {
                    self.pending_timers.borrow_mut().push(TimerEntry {
                        fire_at_ms: next,
                        interval_ms: Some(interval),
                        callback,
                    });
                }
            }
        }
    }

    /// Return the HTML captured via `document.write`/`writeln` so far.
    pub fn document_html(&self) -> String {
        self.doc_buf.borrow().clone()
    }

    fn tick(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err("step budget exceeded".into());
        }
        Ok(())
    }

    // ===== Statement execution =====

    fn exec_block(&mut self, stmts: &[Stmt], env: &ScopeRef) -> Result<Flow, String> {
        // Hoist function declarations so they can be called before definition.
        let hoist: Vec<(&str, &Vec<String>, &Vec<Stmt>)> = stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Func { name, params, body } => Some((name.as_str(), params, body)),
                _ => None,
            })
            .collect();
        for (name, params, body) in hoist {
            let cl = Closure {
                name: Some(name.to_string()),
                params: params.to_vec(),
                body: body.to_vec(),
                env: env.clone(),
            };
            env.borrow_mut()
                .vars
                .insert(name.to_string(), Value::Func(Rc::new(cl)));
        }

        for s in stmts {
            match self.exec_stmt(s, env)? {
                Flow::Normal => {}
                f => return Ok(f),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, s: &Stmt, env: &ScopeRef) -> Result<Flow, String> {
        self.tick()?;
        match s {
            Stmt::Empty => Ok(Flow::Normal),
            Stmt::Block(body) => {
                let child = Scope::child(env);
                self.exec_block(body, &child)
            }
            Stmt::Var { decls, .. } => {
                for (name, init) in decls {
                    let v = match init {
                        Some(e) => self.eval(e, env)?,
                        None => Value::Undefined,
                    };
                    env.borrow_mut().vars.insert(name.clone(), v);
                }
                Ok(Flow::Normal)
            }
            Stmt::Expr(e) => {
                self.eval(e, env)?;
                Ok(Flow::Normal)
            }
            Stmt::If { cond, then, else_ } => {
                if truthy(&self.eval(cond, env)?) {
                    self.exec_stmt(then, env)
                } else if let Some(els) = else_ {
                    self.exec_stmt(els, env)
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::While { cond, body } => {
                while truthy(&self.eval(cond, env)?) {
                    self.tick()?;
                    match self.exec_stmt(body, env)? {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Normal => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                init,
                cond,
                update,
                body,
            } => {
                let loop_env = Scope::child(env);
                if let Some(init_stmt) = init {
                    self.exec_stmt(init_stmt, &loop_env)?;
                }
                loop {
                    self.tick()?;
                    if let Some(c) = cond {
                        if !truthy(&self.eval(c, &loop_env)?) {
                            break;
                        }
                    }
                    match self.exec_stmt(body, &loop_env)? {
                        Flow::Break => break,
                        Flow::Continue => {}
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Normal => {}
                    }
                    if let Some(u) = update {
                        self.eval(u, &loop_env)?;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::ForOf { name, iter, body } => {
                let iter_val = self.eval(iter, env)?;
                let items = match &iter_val {
                    Value::Array(arr) => arr.borrow().clone(),
                    Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    _ => Vec::new(),
                };
                for item in items {
                    self.tick()?;
                    let loop_env = Scope::child(env);
                    loop_env.borrow_mut().vars.insert(name.clone(), item);
                    match self.exec_stmt(body, &loop_env)? {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Normal => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Func { name, params, body } => {
                let cl = Closure {
                    name: Some(name.clone()),
                    params: params.clone(),
                    body: body.clone(),
                    env: env.clone(),
                };
                env.borrow_mut()
                    .vars
                    .insert(name.clone(), Value::Func(Rc::new(cl)));
                Ok(Flow::Normal)
            }
            Stmt::Return(val) => {
                let v = match val {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Undefined,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Throw(e) => {
                let v = self.eval(e, env)?;
                Err(format!("throw: {}", to_string(&v)))
            }
        }
    }

    // ===== Expression evaluation =====

    fn eval(&mut self, e: &Expr, env: &ScopeRef) -> Result<Value, String> {
        self.tick()?;
        match e {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::This => Ok(Value::Undefined),
            Expr::Ident(name) => Ok(env
                .borrow()
                .get(name)
                .unwrap_or(Value::Undefined)),
            Expr::Template(parts) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        TmplPart::Text(t) => out.push_str(t),
                        TmplPart::Expr(ex) => {
                            let v = self.eval(ex, env)?;
                            out.push_str(&to_string(&v));
                        }
                    }
                }
                Ok(Value::Str(out))
            }
            Expr::Array(items) => {
                let mut v = Vec::with_capacity(items.len());
                for it in items {
                    v.push(self.eval(it, env)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(v))))
            }
            Expr::Object(props) => {
                let mut obj = ObjectData::new();
                for (k, ve) in props {
                    obj.props.insert(k.clone(), self.eval(ve, env)?);
                }
                Ok(Value::Object(Rc::new(RefCell::new(obj))))
            }
            Expr::Func(name, params, body) => {
                let cl = Closure {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    env: env.clone(),
                };
                Ok(Value::Func(Rc::new(cl)))
            }
            Expr::Unary(op, operand) => {
                let v = self.eval(operand, env)?;
                Ok(match op {
                    UnOp::Neg => Value::Number(-to_number(&v)),
                    UnOp::Not => Value::Bool(!truthy(&v)),
                    UnOp::Typeof => Value::Str(typeof_val(&v)),
                })
            }
            Expr::Binary(op, a, b) => {
                let av = self.eval(a, env)?;
                let bv = self.eval(b, env)?;
                Ok(self.binary(*op, &av, &bv)?)
            }
            Expr::Logical(op, a, b) => {
                let av = self.eval(a, env)?;
                Ok(match op {
                    LogOp::And => {
                        if !truthy(&av) {
                            av
                        } else {
                            self.eval(b, env)?
                        }
                    }
                    LogOp::Or => {
                        if truthy(&av) {
                            av
                        } else {
                            self.eval(b, env)?
                        }
                    }
                })
            }
            Expr::Ternary(c, a, b) => {
                if truthy(&self.eval(c, env)?) {
                    self.eval(a, env)
                } else {
                    self.eval(b, env)
                }
            }
            Expr::Seq(items) => {
                let mut last = Value::Undefined;
                for it in items {
                    last = self.eval(it, env)?;
                }
                Ok(last)
            }
            Expr::Assign(target, op, value) => {
                let rv = self.eval(value, env)?;
                let new_val = match op {
                    AssignOp::Assign => rv,
                    AssignOp::AddAssign => {
                        let cur = self.eval(target, env)?;
                        self.binary(BinOp::Add, &cur, &rv)?
                    }
                    AssignOp::SubAssign => {
                        let cur = self.eval(target, env)?;
                        self.binary(BinOp::Sub, &cur, &rv)?
                    }
                    AssignOp::MulAssign => {
                        let cur = self.eval(target, env)?;
                        self.binary(BinOp::Mul, &cur, &rv)?
                    }
                    AssignOp::DivAssign => {
                        let cur = self.eval(target, env)?;
                        self.binary(BinOp::Div, &cur, &rv)?
                    }
                    AssignOp::ModAssign => {
                        let cur = self.eval(target, env)?;
                        self.binary(BinOp::Mod, &cur, &rv)?
                    }
                };
                self.assign_target(target, new_val.clone(), env)?;
                Ok(new_val)
            }
            Expr::Member(target, name) => {
                let tv = self.eval(target, env)?;
                Ok(self.get_property(&tv, name))
            }
            Expr::Index(target, idx) => {
                let tv = self.eval(target, env)?;
                let iv = self.eval(idx, env)?;
                Ok(self.get_index(&tv, &iv))
            }
            Expr::Call(callee, args) => self.eval_call(callee, args, env),
            Expr::New(callee, args) => {
                // Best-effort construction: treat like a call, falling back to
                // an empty object for unknown constructors.
                let cv = match callee.as_ref() {
                    Expr::Ident(name) => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = cv {
                    let arg_vals = self.eval_args(args, env)?;
                    return Ok(self.construct(&name, &arg_vals));
                }
                self.eval_call(callee, args, env)
            }
        }
    }

    fn eval_args(&mut self, args: &[Expr], env: &ScopeRef) -> Result<Vec<Value>, String> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a, env)?);
        }
        Ok(out)
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr], env: &ScopeRef) -> Result<Value, String> {
        // Method call: obj.method(args) — dispatch to primitive methods or to a
        // function-valued property of an object.
        if let Expr::Member(target, name) = callee {
            let recv = self.eval(target, env)?;
            let argv = self.eval_args(args, env)?;
            return Ok(self.call_method(recv, name, &argv)?);
        }

        let cv = self.eval(callee, env)?;
        let argv = self.eval_args(args, env)?;
        self.call_value(cv, &argv)
    }

    fn call_value(&mut self, callee: Value, args: &[Value]) -> Result<Value, String> {
        self.tick()?;
        match callee {
            Value::Native(f) => f(args),
            Value::Func(cl) => {
                let scope = Scope::child(&cl.env);
                for (i, p) in cl.params.iter().enumerate() {
                    if p == "rest" {
                        let rest = args.iter().skip(i).cloned().collect::<Vec<_>>();
                        scope
                            .borrow_mut()
                            .vars
                            .insert(p.clone(), Value::Array(Rc::new(RefCell::new(rest))));
                    } else {
                        let v = args.get(i).cloned().unwrap_or(Value::Undefined);
                        scope.borrow_mut().vars.insert(p.clone(), v);
                    }
                }
                match self.exec_block(&cl.body, &scope)? {
                    Flow::Return(v) => Ok(v),
                    _ => Ok(Value::Undefined),
                }
            }
            other => Err(format!("value is not a function: {}", typeof_val(&other))),
        }
    }

    fn construct(&mut self, name: &str, args: &[Value]) -> Value {
        match name {
            "Array" => {
                let arr = match args.get(0) {
                    Some(Value::Number(n)) if args.len() == 1 => {
                        vec![Value::Undefined; *n as usize]
                    }
                    _ => args.to_vec(),
                };
                Value::Array(Rc::new(RefCell::new(arr)))
            }
            "Object" => Value::Object(Rc::new(RefCell::new(ObjectData::new()))),
            "String" => Value::Str(args.first().map_or(String::new(), |v| to_string(v))),
            "Number" => Value::Number(args.first().map_or(f64::NAN, to_number)),
            "Boolean" => Value::Bool(args.first().map_or(false, |v| truthy(v))),
            "Date" => Value::Object(Rc::new(RefCell::new(ObjectData::new()))),
            "RegExp" => Value::Object(Rc::new(RefCell::new(ObjectData::new()))),
            _ => Value::Object(Rc::new(RefCell::new(ObjectData::new()))),
        }
    }

    fn assign_target(
        &mut self,
        target: &Expr,
        value: Value,
        env: &ScopeRef,
    ) -> Result<(), String> {
        match target {
            Expr::Ident(name) => {
                env.borrow_mut().assign(name, value);
                Ok(())
            }
            Expr::Member(obj, name) => {
                let ov = self.eval(obj, env)?;
                if let Value::Object(o) = ov {
                    o.borrow_mut().props.insert(name.clone(), value);
                    return Ok(());
                }
                if let Value::Array(a) = ov {
                    if name == "length" {
                        if let Value::Number(n) = &value {
                            a.borrow_mut().resize(*n as usize, Value::Undefined);
                        }
                        return Ok(());
                    }
                }
                Ok(())
            }
            Expr::Index(obj, idx) => {
                let ov = self.eval(obj, env)?;
                let iv = self.eval(idx, env)?;
                match ov {
                    Value::Array(a) => {
                        if let Value::Number(i) = iv {
                            let mut arr = a.borrow_mut();
                            let i = i as usize;
                            if i >= arr.len() {
                                arr.resize(i + 1, Value::Undefined);
                            }
                            arr[i] = value;
                        }
                    }
                    Value::Object(o) => {
                        o.borrow_mut().props.insert(to_string(&iv), value);
                    }
                    _ => {}
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn binary(&self, op: BinOp, a: &Value, b: &Value) -> Result<Value, String> {
        Ok(match op {
            BinOp::Add => {
                if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
                    Value::Str(format!("{}{}", to_string(a), to_string(b)))
                } else {
                    Value::Number(to_number(a) + to_number(b))
                }
            }
            BinOp::Sub => Value::Number(to_number(a) - to_number(b)),
            BinOp::Mul => Value::Number(to_number(a) * to_number(b)),
            BinOp::Div => Value::Number(to_number(a) / to_number(b)),
            BinOp::Mod => Value::Number(to_number(a) % to_number(b)),
            BinOp::Eq => Value::Bool(loose_eq(a, b)),
            BinOp::Ne => Value::Bool(!loose_eq(a, b)),
            BinOp::StrictEq => Value::Bool(strict_eq(a, b)),
            BinOp::StrictNe => Value::Bool(!strict_eq(a, b)),
            BinOp::Lt => Value::Bool(compare(a, b).map_or(false, |o| o == std::cmp::Ordering::Less)),
            BinOp::Gt => Value::Bool(compare(a, b).map_or(false, |o| o == std::cmp::Ordering::Greater)),
            BinOp::Le => Value::Bool(compare(a, b).map_or(false, |o| o != std::cmp::Ordering::Greater)),
            BinOp::Ge => Value::Bool(compare(a, b).map_or(false, |o| o != std::cmp::Ordering::Less)),
        })
    }
}

// ===== Property / index access =====

impl Interpreter {
    fn get_property(&self, v: &Value, name: &str) -> Value {
        match v {
            Value::Str(s) => {
                if name == "length" {
                    return Value::Number(s.chars().count() as f64);
                }
                Value::Undefined
            }
            Value::Array(a) => {
                if name == "length" {
                    return Value::Number(a.borrow().len() as f64);
                }
                Value::Undefined
            }
            Value::Object(o) => o.borrow().props.get(name).cloned().unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        }
    }

    fn get_index(&self, v: &Value, idx: &Value) -> Value {
        match v {
            Value::Str(s) => {
                if let Value::Number(i) = idx {
                    let i = *i as i64;
                    if i >= 0 {
                        if let Some(c) = s.chars().nth(i as usize) {
                            return Value::Str(c.to_string());
                        }
                    }
                }
                Value::Undefined
            }
            Value::Array(a) => {
                if let Value::Number(i) = idx {
                    return a.borrow().get(*i as usize).cloned().unwrap_or(Value::Undefined);
                }
                if let Value::Str(key) = idx {
                    if key == "length" {
                        return Value::Number(a.borrow().len() as f64);
                    }
                }
                Value::Undefined
            }
            Value::Object(o) => {
                o.borrow().props.get(&to_string(idx)).cloned().unwrap_or(Value::Undefined)
            }
            _ => Value::Undefined,
        }
    }
}

// ===== Method dispatch =====

impl Interpreter {
    fn call_method(&mut self, recv: Value, name: &str, args: &[Value]) -> Result<Value, String> {
        match &recv {
            Value::Str(s) => Ok(string_method(s, name, args)),
            Value::Array(a) => self.array_method(a.clone(), name, args),
            Value::Number(n) => Ok(number_method(*n, name, args)),
            Value::Object(o) => {
                let func = o.borrow().props.get(name).cloned();
                match func {
                    Some(Value::Native(_)) | Some(Value::Func(_)) => self.call_value(func.unwrap(), args),
                    _ => Ok(Value::Undefined),
                }
            }
            _ => Ok(Value::Undefined),
        }
    }
}

// ===== Global environment setup =====

impl Interpreter {
    fn install_globals(
        &self,
        global: &ScopeRef,
        doc_buf: &Rc<RefCell<String>>,
        pending_timers: &Rc<RefCell<Vec<TimerEntry>>>,
    ) {
        let mut g = global.borrow_mut();

        // document host object
        let mut document = ObjectData::new();
        let buf = doc_buf.clone();
        document.props.insert(
            "write".into(),
            Value::Native(Rc::new(move |args: &[Value]| {
                let mut b = buf.borrow_mut();
                for a in args {
                    b.push_str(&to_string(a));
                }
                Ok(Value::Undefined)
            }) as NativeFn),
        );
        let buf = doc_buf.clone();
        document.props.insert(
            "writeln".into(),
            Value::Native(Rc::new(move |args: &[Value]| {
                let mut b = buf.borrow_mut();
                for a in args {
                    b.push_str(&to_string(a));
                }
                b.push('\n');
                Ok(Value::Undefined)
            }) as NativeFn),
        );
        document
            .props
            .insert("getElementById".into(), Value::Native(Rc::new(|_| Ok(Value::Null))));
        document
            .props
            .insert("querySelector".into(), Value::Native(Rc::new(|_| Ok(Value::Null))));
        document
            .props
            .insert("querySelectorAll".into(), Value::Native(Rc::new(|_| Ok(empty_array()))));
        document.props.insert(
            "createElement".into(),
            Value::Native(Rc::new(|_| Ok(empty_element()))),
        );
        document
            .props
            .insert("createTextNode".into(), Value::Native(Rc::new(|_| Ok(empty_element()))));
        g.vars.insert("document".into(), Value::Object(Rc::new(RefCell::new(document))));

        // console (no-op)
        let mut console = ObjectData::new();
        let noop: NativeFn = Rc::new(|_| Ok(Value::Undefined));
        for m in ["log", "info", "warn", "error", "debug", "trace"] {
            console.props.insert(m.into(), Value::Native(noop.clone()));
        }
        g.vars.insert("console".into(), Value::Object(Rc::new(RefCell::new(console))));

        // Math
        let mut math = ObjectData::new();
        math.props.insert("PI".into(), Value::Number(std::f64::consts::PI));
        math.props.insert("E".into(), Value::Number(std::f64::consts::E));
        for (k, f) in math_methods() {
            math.props.insert(k.into(), Value::Native(f));
        }
        g.vars.insert("Math".into(), Value::Object(Rc::new(RefCell::new(math))));

        // JSON
        let mut json = ObjectData::new();
        json.props.insert("parse".into(), Value::Native(Rc::new(json_parse)));
        json.props.insert("stringify".into(), Value::Native(Rc::new(json_stringify)));
        g.vars.insert("JSON".into(), Value::Object(Rc::new(RefCell::new(json))));

        // Global constructors and functions
        g.vars.insert(
            "String".into(),
            Value::Native(Rc::new(|args| {
                Ok(Value::Str(args.first().map_or(String::new(), to_string)))
            })),
        );
        g.vars.insert(
            "Number".into(),
            Value::Native(Rc::new(|args| Ok(Value::Number(args.first().map_or(f64::NAN, to_number))))),
        );
        g.vars.insert(
            "Boolean".into(),
            Value::Native(Rc::new(|args| Ok(Value::Bool(args.first().map_or(false, |v| truthy(v)))))),
        );
        g.vars.insert(
            "Array".into(),
            Value::Native(Rc::new(|args| {
                Ok(Value::Array(Rc::new(RefCell::new(args.to_vec()))))
            })),
        );
        g.vars.insert(
            "Object".into(),
            Value::Native(Rc::new(|_| Ok(Value::Object(Rc::new(RefCell::new(ObjectData::new())))))),
        );
        g.vars.insert(
            "parseInt".into(),
            Value::Native(Rc::new(|args| Ok(Value::Number(parse_int(args.first()))))),
        );
        g.vars.insert(
            "parseFloat".into(),
            Value::Native(Rc::new(|args| Ok(Value::Number(parse_float(args.first()))))),
        );
        g.vars.insert(
            "isNaN".into(),
            Value::Native(Rc::new(|args| Ok(Value::Bool(to_number(args.first().unwrap_or(&Value::Undefined)).is_nan())))),
        );
        g.vars.insert(
            "isFinite".into(),
            Value::Native(Rc::new(|args| {
                let n = to_number(args.first().unwrap_or(&Value::Undefined));
                Ok(Value::Bool(n.is_finite()))
            })),
        );
        let timers_ref = pending_timers.clone();
        g.vars.insert(
            "setTimeout".into(),
            Value::Native(Rc::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let delay = to_number(args.get(1).unwrap_or(&Value::Number(0.0))).max(0.0) as u64;
                if matches!(cb, Value::Func(_) | Value::Native(_)) {
                    timers_ref.borrow_mut().push(TimerEntry {
                        fire_at_ms: delay,
                        interval_ms: None,
                        callback: cb,
                    });
                }
                Ok(Value::Undefined)
            })),
        );
        let timers_ref = pending_timers.clone();
        g.vars.insert(
            "setInterval".into(),
            Value::Native(Rc::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let interval = to_number(args.get(1).unwrap_or(&Value::Number(0.0))).max(0.0) as u64;
                if interval == 0 {
                    return Ok(Value::Undefined);
                }
                if matches!(cb, Value::Func(_) | Value::Native(_)) {
                    timers_ref.borrow_mut().push(TimerEntry {
                        fire_at_ms: interval,
                        interval_ms: Some(interval),
                        callback: cb,
                    });
                }
                Ok(Value::Undefined)
            })),
        );
        let timers_ref = pending_timers.clone();
        g.vars.insert(
            "requestAnimationFrame".into(),
            Value::Native(Rc::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                if matches!(cb, Value::Func(_) | Value::Native(_)) {
                    timers_ref.borrow_mut().push(TimerEntry {
                        fire_at_ms: 16,
                        interval_ms: None,
                        callback: cb,
                    });
                }
                Ok(Value::Undefined)
            })),
        );
        g.vars.insert("undefined".into(), Value::Undefined);
        g.vars.insert("NaN".into(), Value::Number(f64::NAN));
        g.vars.insert("Infinity".into(), Value::Number(f64::INFINITY));
    }
}

// ===== Value helpers (free functions) =====

fn empty_array() -> Value {
    Value::Array(Rc::new(RefCell::new(Vec::new())))
}

fn empty_element() -> Value {
    let mut obj = ObjectData::new();
    obj.props.insert("innerHTML".into(), Value::Str(String::new()));
    obj.props.insert("textContent".into(), Value::Str(String::new()));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        _ => true,
    }
}

fn to_number(v: &Value) -> f64 {
    match v {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => *n,
        Value::Str(s) => s.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn to_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Number(n) => num_to_string(*n),
        Value::Str(s) => s.clone(),
        Value::Array(a) => a
            .borrow()
            .iter()
            .map(|e| match e {
                Value::Undefined | Value::Null => String::new(),
                _ => to_string(e),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
        Value::Func(cl) => format!(
            "function {}() {{}}",
            cl.name.as_deref().unwrap_or("")
        ),
        Value::Native(_) => "function () { [native code] }".into(),
    }
}

fn typeof_val(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".into(),
        Value::Null => "object".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Number(_) => "number".into(),
        Value::Str(_) => "string".into(),
        Value::Array(_) | Value::Object(_) => "object".into(),
        Value::Func(_) | Value::Native(_) => "function".into(),
    }
}

fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => Rc::ptr_eq(x, y),
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Null) | (Value::Null, Value::Undefined) => true,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(_), Value::Str(_)) => to_number(a) == to_number(b),
        (Value::Str(_), Value::Number(_)) => to_number(a) == to_number(b),
        (Value::Bool(_), _) => loose_eq(&Value::Number(to_number(a)), b),
        (_, Value::Bool(_)) => loose_eq(a, &Value::Number(to_number(b))),
        _ => strict_eq(a, b),
    }
}

fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        _ => {
            let x = to_number(a);
            let y = to_number(b);
            x.partial_cmp(&y)
        }
    }
    .map(|o| {
        if o == Ordering::Equal && (to_number(a).is_nan() || to_number(b).is_nan()) {
            Ordering::Equal
        } else {
            o
        }
    })
}

// ===== String methods =====

fn string_method(s: &str, name: &str, args: &[Value]) -> Value {
    let arg0 = || args.first().map_or(String::new(), to_string);
    let arg0_n = || to_number(args.first().unwrap_or(&Value::Undefined)) as i64;
    match name {
        "toString" | "valueOf" => Value::Str(s.to_string()),
        "toUpperCase" => Value::Str(s.to_uppercase()),
        "toLowerCase" => Value::Str(s.to_lowercase()),
        "trim" => Value::Str(s.trim().to_string()),
        "trimStart" | "trimLeft" => Value::Str(s.trim_start().to_string()),
        "trimEnd" | "trimRight" => Value::Str(s.trim_end().to_string()),
        "charAt" => {
            let i = arg0_n();
            Value::Str(
                s.chars()
                    .nth(i.max(0) as usize)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            )
        }
        "charCodeAt" => {
            let i = arg0_n();
            Value::Number(
                s.chars()
                    .nth(i.max(0) as usize)
                    .map(|c| c as u32 as f64)
                    .unwrap_or(f64::NAN),
            )
        }
        "indexOf" => {
            let needle = arg0();
            Value::Number(s.find(&needle).map(|p| byte_to_char_idx(s, p) as f64).unwrap_or(-1.0))
        }
        "lastIndexOf" => {
            let needle = arg0();
            Value::Number(s.rfind(&needle).map(|p| byte_to_char_idx(s, p) as f64).unwrap_or(-1.0))
        }
        "includes" => Value::Bool(s.contains(&arg0())),
        "startsWith" => Value::Bool(s.starts_with(&arg0())),
        "endsWith" => Value::Bool(s.ends_with(&arg0())),
        "slice" => Value::Str(slice_str(s, args)),
        "substring" | "substr" => Value::Str(slice_str(s, args)),
        "split" => {
            let sep = match args.first() {
                Some(Value::Undefined) => return Value::Array(Rc::new(RefCell::new(vec![Value::Str(s.to_string())]))),
                Some(v) => to_string(v),
                None => return Value::Array(Rc::new(RefCell::new(vec![Value::Str(s.to_string())]))),
            };
            if sep.is_empty() {
                let parts: Vec<Value> = s.chars().map(|c| Value::Str(c.to_string())).collect();
                Value::Array(Rc::new(RefCell::new(parts)))
            } else {
                let parts: Vec<Value> = s.split(&sep).map(|p| Value::Str(p.to_string())).collect();
                Value::Array(Rc::new(RefCell::new(parts)))
            }
        }
        "replace" => {
            let from = arg0();
            let with = args.get(1).map_or(String::new(), to_string);
            Value::Str(s.replacen(&from, &with, 1))
        }
        "replaceAll" => {
            let from = arg0();
            let with = args.get(1).map_or(String::new(), to_string);
            Value::Str(s.replace(&from, &with))
        }
        "repeat" => {
            let n = arg0_n().max(0) as usize;
            Value::Str(s.repeat(n))
        }
        "concat" => {
            let mut out = s.to_string();
            for a in args {
                out.push_str(&to_string(a));
            }
            Value::Str(out)
        }
        "padStart" | "padEnd" => {
            let len = arg0_n().max(0) as usize;
            let pad = args.get(1).map_or(" ".to_string(), to_string);
            Value::Str(pad_str(s, len, &pad, name == "padStart"))
        }
        "at" => {
            let mut i = arg0_n();
            let chars: Vec<char> = s.chars().collect();
            if i < 0 {
                i += chars.len() as i64;
            }
            Value::Str(
                chars
                    .get(i as usize)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            )
        }
        _ => Value::Undefined,
    }
}

fn byte_to_char_idx(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].chars().count()
}

fn slice_str(s: &str, args: &[Value]) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    let start = to_number(args.first().unwrap_or(&Value::Undefined));
    let start = if start.is_nan() {
        0
    } else {
        normalize(start as i64, len)
    };
    let end = match args.get(1) {
        Some(Value::Undefined) | None => len,
        Some(v) => {
            let n = to_number(v);
            if n.is_nan() {
                len
            } else {
                normalize(n as i64, len)
            }
        }
    };
    let (a, b) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    chars[a as usize..(b as usize).min(chars.len())]
        .iter()
        .collect()
}

fn normalize(i: i64, len: i64) -> i64 {
    if i < 0 {
        (len + i).max(0)
    } else {
        i.min(len)
    }
}

fn pad_str(s: &str, len: usize, pad: &str, at_start: bool) -> String {
    if pad.is_empty() || s.chars().count() >= len {
        return s.to_string();
    }
    let need = len - s.chars().count();
    let pad_chars: Vec<char> = pad.chars().collect();
    let mut filling = String::new();
    for i in 0..need {
        filling.push(pad_chars[i % pad_chars.len()]);
    }
    if at_start {
        format!("{filling}{s}")
    } else {
        format!("{s}{filling}")
    }
}

// ===== Number methods =====

fn number_method(n: f64, name: &str, args: &[Value]) -> Value {
    match name {
        "toString" => {
            let radix = to_number(args.first().unwrap_or(&Value::Number(10.0))) as u32;
            if radix == 10 || args.first().is_none() {
                Value::Str(num_to_string(n))
            } else if (2..=36).contains(&radix) {
                Value::Str(format_radix(n as i64, radix))
            } else {
                Value::Str(num_to_string(n))
            }
        }
        "toFixed" => {
            let d = to_number(args.first().unwrap_or(&Value::Number(0.0))) as usize;
            Value::Str(format!("{:.*}", d, n))
        }
        "valueOf" => Value::Number(n),
        _ => Value::Undefined,
    }
}

fn format_radix(n: i64, radix: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let neg = n < 0;
    let mut n = n.unsigned_abs();
    let mut out = Vec::new();
    while n > 0 {
        let d = (n % radix as u64) as u32;
        let c = std::char::from_digit(d, radix).unwrap_or('0');
        out.push(c);
        n /= radix as u64;
    }
    if neg {
        out.push('-');
    }
    out.iter().rev().collect()
}

// ===== Array methods =====

impl Interpreter {
    fn array_method(
        &mut self,
        arr: Rc<RefCell<Vec<Value>>>,
        name: &str,
        args: &[Value],
    ) -> Result<Value, String> {
        match name {
            "push" => {
                let mut a = arr.borrow_mut();
                let new_len = a.len() + args.len();
                for v in args {
                    a.push(v.clone());
                }
                Ok(Value::Number(new_len as f64))
            }
            "pop" => Ok(arr.borrow_mut().pop().unwrap_or(Value::Undefined)),
            "shift" => {
                let mut a = arr.borrow_mut();
                if a.is_empty() {
                    Ok(Value::Undefined)
                } else {
                    Ok(a.remove(0))
                }
            }
            "unshift" => {
                let mut a = arr.borrow_mut();
                for (i, v) in args.iter().enumerate() {
                    a.insert(i, v.clone());
                }
                Ok(Value::Number(a.len() as f64))
            }
            "join" => {
                let sep = match args.first() {
                    Some(Value::Undefined) | None => ",".to_string(),
                    Some(v) => to_string(v),
                };
                let a = arr.borrow();
                let parts: Vec<String> = a
                    .iter()
                    .map(|e| match e {
                        Value::Undefined | Value::Null => String::new(),
                        _ => to_string(e),
                    })
                    .collect();
                Ok(Value::Str(parts.join(&sep)))
            }
            "concat" => {
                let mut out = arr.borrow().clone();
                for a in args {
                    match a {
                        Value::Array(other) => out.extend(other.borrow().iter().cloned()),
                        _ => out.push(a.clone()),
                    }
                }
                Ok(Value::Array(Rc::new(RefCell::new(out))))
            }
            "slice" => {
                let a = arr.borrow().clone();
                let len = a.len() as i64;
                let start = normalize(
                    to_number(args.first().unwrap_or(&Value::Undefined)) as i64,
                    len,
                );
                let end = match args.get(1) {
                    Some(Value::Undefined) | None => len,
                    Some(v) => normalize(to_number(v) as i64, len),
                };
                let (s, e) = (start.min(end), start.max(end));
                Ok(Value::Array(Rc::new(RefCell::new(
                    a[s as usize..e as usize].to_vec(),
                ))))
            }
            "indexOf" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                for (i, e) in arr.borrow().iter().enumerate() {
                    if strict_eq(e, &needle) {
                        return Ok(Value::Number(i as f64));
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "includes" => {
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                for e in arr.borrow().iter() {
                    if strict_eq(e, &needle) {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            "reverse" => {
                arr.borrow_mut().reverse();
                Ok(Value::Array(arr))
            }
            "map" | "filter" | "forEach" => {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let src = arr.borrow().clone();
                let mut out = Vec::new();
                for (i, item) in src.iter().enumerate() {
                    let cb_args = vec![item.clone(), Value::Number(i as f64), Value::Array(arr.clone())];
                    let ret = self.call_value(cb.clone(), &cb_args)?;
                    match name {
                        "map" => out.push(ret),
                        "filter" => {
                            if truthy(&ret) {
                                out.push(item.clone());
                            }
                        }
                        "forEach" => {}
                        _ => {}
                    }
                }
                match name {
                    "forEach" => Ok(Value::Undefined),
                    _ => Ok(Value::Array(Rc::new(RefCell::new(out)))),
                }
            }
            "toString" => {
                let a = arr.borrow();
                let parts: Vec<String> = a
                    .iter()
                    .map(|e| match e {
                        Value::Undefined | Value::Null => String::new(),
                        _ => to_string(e),
                    })
                    .collect();
                Ok(Value::Str(parts.join(",")))
            }
            _ => Ok(Value::Undefined),
        }
    }
}

// ===== Math =====

/// Lift a closure into a reference-counted native function value.
fn native<F: Fn(&[Value]) -> Result<Value, String> + 'static>(f: F) -> NativeFn {
    Rc::new(f)
}

fn math_methods() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("floor", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).floor()))))),
        ("ceil", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).ceil()))))),
        ("round", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).round()))))),
        ("trunc", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).trunc()))))),
        ("abs", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).abs()))))),
        ("sqrt", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).sqrt()))))),
        ("cbrt", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).cbrt()))))),
        ("sign", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).signum()))))),
        ("log", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).ln()))))),
        ("exp", native(|a| Ok(Value::Number(a.first().map_or(f64::NAN, |v| to_number(v).exp()))))),
        (
            "pow",
            native(|a| {
                let x = to_number(a.first().unwrap_or(&Value::Undefined));
                let y = to_number(a.get(1).unwrap_or(&Value::Undefined));
                Ok(Value::Number(x.powf(y)))
            }),
        ),
        (
            "min",
            native(|a| {
                if a.is_empty() {
                    return Ok(Value::Number(f64::INFINITY));
                }
                Ok(Value::Number(a.iter().map(to_number).fold(f64::INFINITY, f64::min)))
            }),
        ),
        (
            "max",
            native(|a| {
                if a.is_empty() {
                    return Ok(Value::Number(f64::NEG_INFINITY));
                }
                Ok(Value::Number(
                    a.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max),
                ))
            }),
        ),
        ("random", native(|_| Ok(Value::Number(0.5)))),
    ]
}

// ===== Global functions =====

fn parse_int(v: Option<&Value>) -> f64 {
    let s = match v {
        Some(Value::Str(s)) => s.trim_start(),
        Some(Value::Number(n)) => return n.trunc(),
        Some(Value::Bool(b)) => return if *b { 1.0 } else { 0.0 },
        _ => return f64::NAN,
    };
    let mut chars = s.chars();
    let mut sign = 1.0;
    match chars.clone().next() {
        Some('+') => {
            chars.next();
        }
        Some('-') => {
            sign = -1.0;
            chars.next();
        }
        _ => {}
    }
    let digits: String = chars.take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.parse::<f64>().unwrap_or(f64::NAN)
}

fn parse_float(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => *n,
        Some(Value::Str(s)) => {
            let trimmed = s.trim_start();
            let end = trimmed
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '+' || c == '-' || c == 'e' || c == 'E'))
                .unwrap_or(trimmed.len());
            trimmed[..end].parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

// ===== JSON (via serde_json) =====

fn json_parse(args: &[Value]) -> Result<Value, String> {
    let s = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        Some(v) => to_string(v),
        None => return Err("JSON.parse: missing argument".into()),
    };
    let val: serde_json::Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    Ok(from_json(val))
}

fn json_stringify(args: &[Value]) -> Result<Value, String> {
    let v = args.first().cloned().unwrap_or(Value::Undefined);
    let jv = to_json(&v);
    match serde_json::to_string(&jv) {
        Ok(s) => Ok(Value::Str(s)),
        Err(e) => Err(e.to_string()),
    }
}

fn from_json(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => Value::Str(s),
        serde_json::Value::Array(a) => {
            Value::Array(Rc::new(RefCell::new(a.into_iter().map(from_json).collect())))
        }
        serde_json::Value::Object(o) => {
            let mut obj = ObjectData::new();
            for (k, v) in o {
                obj.props.insert(k, from_json(v));
            }
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

fn to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Undefined => serde_json::Value::Null,
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Array(a) => {
            serde_json::Value::Array(a.borrow().iter().map(to_json).collect())
        }
        Value::Object(o) => {
            let mut map = serde_json::Map::new();
            for (k, val) in &o.borrow().props {
                map.insert(k.clone(), to_json(val));
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> String {
        let mut it = Interpreter::new();
        it.run_script(src).unwrap();
        it.document_html()
    }

    #[test]
    fn document_write_string() {
        assert_eq!(doc(r#"document.write("hello");"#), "hello");
    }

    #[test]
    fn document_write_template_and_vars() {
        let out = doc(
            r#"var name = "world";
               document.write(`<h1>Hi ${name}</h1>`);"#,
        );
        assert_eq!(out, "<h1>Hi world</h1>");
    }

    #[test]
    fn document_write_loop() {
        let out = doc(
            r#"
            for (var i = 0; i < 3; i++) {
              document.write("<p>" + i + "</p>");
            }
            "#,
        );
        assert_eq!(out, "<p>0</p><p>1</p><p>2</p>");
    }

    #[test]
    fn document_write_for_of_array() {
        let out = doc(
            r#"
            var items = ["a", "b", "c"];
            for (var x of items) {
              document.write(x);
            }
            "#,
        );
        assert_eq!(out, "abc");
    }

    #[test]
    fn function_declaration_hoisted() {
        let out = doc(
            r#"
            document.write(add(2, 3));
            function add(a, b) { return a + b; }
            "#,
        );
        assert_eq!(out, "5");
    }

    #[test]
    fn string_and_math_builtins() {
        let out = doc(
            r#"
            var s = "Hello".toUpperCase();
            var n = Math.floor(3.9);
            document.write(s + n);
            "#,
        );
        assert_eq!(out, "HELLO3");
    }

    #[test]
    fn json_parse_and_access() {
        let out = doc(
            r#"
            var data = JSON.parse('{"name":"Ann","age":30}');
            document.write(data.name + data.age);
            "#,
        );
        assert_eq!(out, "Ann30");
    }

    #[test]
    fn conditionals_and_logical() {
        let out = doc(
            r#"
            var a = 5;
            if (a > 3 && a < 10) {
              document.write("in range");
            } else {
              document.write("out");
            }
            "#,
        );
        assert_eq!(out, "in range");
    }

    #[test]
    fn unsupported_script_does_not_panic() {
        let mut it = Interpreter::new();
        // Arrow functions are outside the subset and must error, not crash.
        assert!(it.run_script("var f = () => 1;").is_err());
        assert_eq!(it.document_html(), "");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(num_to_string(1.0), "1");
        assert_eq!(num_to_string(3.14), "3.14");
        assert_eq!(num_to_string(f64::NAN), "NaN");
    }
}
