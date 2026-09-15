use super::*;

#[test]
fn simple_paragraph() {
    let html = "<p>Hello world</p>";
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Hello world"));
}

#[test]
fn convert_progressive_emits_blocks() {
    let html = "<body><div><h1>Title</h1><p>One</p><p>Two</p></div></body>";
    let mut blocks = Vec::new();
    PageToMarkdown::convert_progressive(html, false, false, false, &[], |b| {
        blocks.push(b);
    })
    .unwrap();
    assert!(blocks.len() >= 2);
    let joined = blocks.join("\n");
    assert!(joined.contains("Title"));
    assert!(joined.contains("One"));
    assert!(joined.contains("Two"));
}

#[test]
fn heading_conversion() {
    let html = "<h1>Title</h1><h2>Subtitle</h2>";
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Title"));
    assert!(md.contains("Subtitle"));
}

#[test]
fn removes_scripts_and_styles() {
    let html = r#"
        <html>
        <head><style>body{color:red}</style></head>
        <body>
            <script>alert('x')</script>
            <p>Content</p>
        </body>
        </html>
    "#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("alert"));
    assert!(!md.contains("color:red"));
    assert!(md.contains("Content"));
}

#[test]
fn strips_images_when_false() {
    let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("a.png"));
    assert!(!md.contains("pic"));
    assert!(md.contains("Text before"));
    assert!(md.contains("Text after"));
}

#[test]
fn keeps_images_when_true() {
    let html = r#"<p>Text before</p><img src="a.png" alt="pic"><p>Text after</p>"#;
    let md = PageToMarkdown::convert(html, true, false, false, &[]).unwrap();
    assert!(md.contains("a.png"));
    assert!(md.contains("pic"));
    assert!(md.contains("Text before"));
    assert!(md.contains("Text after"));
}

#[test]
fn strips_self_closing_images() {
    let html = r#"<p>Before</p><img src="b.png" alt="self"/><p>After</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("b.png"));
    assert!(!md.contains("self"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

#[test]
fn strips_iframe_tags() {
    let html = r#"
        <p>Before</p>
        <iframe src="https://video.ibm.com/embed/123" allowfullscreen></iframe>
        <p>After</p>
    "#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("iframe"));
    assert!(!md.contains("video.ibm.com"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

#[test]
fn strips_iframe_tags_self_closing() {
    let html = r#"<p>Before</p><iframe src="map.html"/><p>After</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("iframe"));
    assert!(!md.contains("map.html"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

#[test]
fn strips_nav_tags() {
    let html = r#"<nav><a href="/">Home</a><a href="/about">About</a></nav><p>Content</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("Home"));
    assert!(!md.contains("About"));
    assert!(md.contains("Content"));
}

#[test]
fn strips_footer_tags() {
    let html = r#"<p>Article</p><footer>Copyright 2025</footer>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Article"));
    assert!(!md.contains("Copyright"));
}

#[test]
fn strips_aside_tags() {
    let html = r#"<p>Main text</p><aside>Sidebar content</aside>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Main text"));
    assert!(!md.contains("Sidebar"));
}

#[test]
fn strips_noscript_tags() {
    let html = r#"<noscript>Please enable JS</noscript><p>Visible</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("enable JS"));
    assert!(md.contains("Visible"));
}

#[test]
fn strips_form_tags() {
    let html = r#"<form action="/submit"><input type="text"/><button>Go</button></form><p>Text</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("submit"));
    assert!(!md.contains("button"));
    assert!(md.contains("Text"));
}

#[test]
fn strips_html_comments() {
    let html = r#"<p>Before</p><!-- this is a comment --><p>After</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("comment"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

#[test]
fn strips_noise_tags_case_insensitive() {
    let html = r#"<NAV>Menu</NAV><p>Body</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("Menu"));
    assert!(md.contains("Body"));
}

#[test]
fn preserves_content_between_noise_tags() {
    let html = r#"<nav>Nav</nav><p>First</p><footer>Foot</footer><p>Second</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("Nav"));
    assert!(!md.contains("Foot"));
    assert!(md.contains("First"));
    assert!(md.contains("Second"));
}

#[test]
fn code_block_language_preserved() {
    let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("```rust"), "expected language annotation, got: {}", md);
    assert!(md.contains("fn main()"));
}

#[test]
fn code_block_language_python() {
    let html = r#"<pre><code class="language-python">print("hello")</code></pre>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("```python"), "expected language annotation, got: {}", md);
    assert!(md.contains("print"));
}

#[test]
fn code_block_no_language_stays_plain() {
    let html = r#"<pre><code>plain code</code></pre>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("```"));
    assert!(!md.contains("```rust"));
    assert!(!md.contains("```python"));
    assert!(md.contains("plain code"));
}

#[test]
fn multiple_code_blocks_with_languages() {
    let html = r#"<pre><code class="language-rust">let x = 1;</code></pre><p>text</p><pre><code class="language-go">fmt.Println()</code></pre>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("```rust"));
    assert!(md.contains("```go"));
    assert!(md.contains("let x = 1;"));
    assert!(md.contains("fmt.Println()"));
}

#[test]
fn strips_header_tags_by_default() {
    let html = r#"<header><h1>Site Title</h1><nav>Menu</nav></header><p>Article body</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("Site Title"));
    assert!(!md.contains("Menu"));
    assert!(md.contains("Article body"));
}

#[test]
fn keeps_header_when_requested() {
    let html = r#"<header><h1>Article Title</h1></header><p>Article body</p>"#;
    let md = PageToMarkdown::convert(html, false, true, false, &[]).unwrap();
    assert!(md.contains("Article Title"));
    assert!(md.contains("Article body"));
}

#[test]
fn dedup_removes_duplicate_paragraphs() {
    let html = r#"<p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>This is a long paragraph that should be deduplicated when it appears twice.</p><p>Unique content here.</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    let count = md.matches("deduplicated").count();
    assert_eq!(count, 1, "duplicate paragraph should appear once, got: {}", md);
    assert!(md.contains("Unique content"));
}

#[test]
fn dedup_keeps_short_blocks() {
    let html = r#"<h1>Title</h1><h1>Title</h1><p>Body text</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Title"));
    assert!(md.contains("Body text"));
}

#[test]
fn dedup_preserves_unique_blocks() {
    let html = r#"<p>First unique paragraph with enough text to exceed the threshold.</p><p>Second unique paragraph with different content entirely.</p><p>Third unique paragraph also different from the rest.</p>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("First unique"));
    assert!(md.contains("Second unique"));
    assert!(md.contains("Third unique"));
}

#[test]
fn main_content_extracts_article_tag() {
    let html = r#"<nav>Home About</nav><article><h1>Article Title</h1><p>This is the main article content that should be extracted.</p></article><footer>Copyright 2025</footer>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("Article Title"));
    assert!(md.contains("main article content"));
    assert!(!md.contains("Copyright"));
}

#[test]
fn main_content_extracts_main_tag() {
    let html = r#"<div>Sidebar noise</div><main><p>Main content goes here with enough text to be meaningful.</p></main><aside>Related links</aside>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("Main content"));
    assert!(!md.contains("Sidebar noise"));
}

#[test]
fn main_content_extracts_role_main_div() {
    let html = r#"<div class="sidebar">Sidebar</div><div role="main"><p>This is the main content extracted via role attribute.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("role attribute"));
    assert!(!md.contains("Sidebar"));
}

#[test]
fn main_content_falls_back_to_full_html() {
    let html = r#"<div><p>Just some content without semantic tags.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("Just some content"));
}

#[test]
fn generic_page_skips_main_content_without_flag() {
    // Non-article pages should keep surrounding content when --main-content is off.
    let html = r#"<div class="wrap"><p>Intro blurb outside any semantic main region that should remain for generic pages.</p>
        <div><p>More body text on a generic page that is not classified as article or product.</p></div></div>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("Intro blurb"));
    assert!(md.contains("More body text"));
    assert_eq!(PageToMarkdown::detect_page_type(html), "page");
}

#[test]
fn fallback_chain_picks_highest_scoring_candidate() {
    let html = r#"<article><p>Short.</p></article><div><p>This div block has substantially more article text than the short article tag above, so the fallback chain should prefer it as the main content candidate when scoring all sources together.</p><p>Another paragraph adds even more text density to ensure this block wins over the semantic article wrapper.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("substantially more article text"));
    assert!(!md.contains("Short."));
}

#[test]
fn boilerplate_strips_link_heavy_paragraphs() {
    let html = r#"<article><p><a href="/a">Link A</a> <a href="/b">Link B</a> <a href="/c">Link C</a></p><p>This is substantial article content with enough text to survive boilerplate stripping and represent the real main body of the page for extraction purposes.</p></article>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("substantial article content"));
    assert!(!md.contains("Link A"));
    assert!(!md.contains("Link B"));
}

#[test]
fn structured_metadata_fallback_when_heuristics_fail() {
    let html = r#"<div><nav><a href="/">Home</a><a href="/about">About</a></nav></div>
    <script type="application/ld+json">{"@type":"NewsArticle","articleBody":"<p>Structured article body from JSON-LD metadata that should be extracted when DOM heuristics cannot find a high-scoring content block on this page.</p>"}</script>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("Structured article body from JSON-LD"));
    assert!(!md.contains("About"));
}

#[test]
fn readability_extracts_content_div_from_layout() {
    let html = r#"<div><a href="/">Home</a><a href="/about">About</a><a href="/contact">Contact</a><a href="/blog">Blog</a><a href="/shop">Shop</a></div><div><h2>Article Title</h2><p>This is a substantial article body with enough text content to score well in the readability algorithm. It contains meaningful paragraphs of text that should be extracted as the main content of the page, far exceeding the navigation links above.</p><p>Another paragraph with additional content to further increase the text density score of this content block compared to the navigation block which consists mostly of short link texts.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("substantial article body"));
    assert!(!md.contains("Shop"));
}

#[test]
fn readability_falls_back_when_no_semantic_tags() {
    let html = r#"<div><a href="/">Home</a><a href="/about">About</a></div><div><p>This is the main content paragraph with enough text to exceed the readability threshold for extraction. It has substantial body text that should be identified as the primary content block by the scoring algorithm.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("main content paragraph"));
    assert!(!md.contains("About"));
}

#[test]
fn readability_returns_full_html_when_no_good_candidate() {
    let html = r#"<div><p>Short.</p></div><div><p>Brief.</p></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("Short") || md.contains("Brief"));
}

#[test]
fn readability_prefers_text_over_navigation() {
    let nav_html = r#"<div><a href="/a">Link A</a><a href="/b">Link B</a><a href="/c">Link C</a></div>"#;
    let content_html = r#"<div><p>This block has substantial text content that should score higher than the navigation block which only contains short link texts. The readability algorithm should correctly identify this as the main content area of the page.</p></div>"#;
    let html = format!("{}{}", nav_html, content_html);
    let md = PageToMarkdown::convert(&html, false, false, true, &[]).unwrap();
    assert!(md.contains("substantial text content"));
    assert!(!md.contains("Link A"));
    assert!(!md.contains("Link B"));
    assert!(!md.contains("Link C"));
}

#[test]
fn readability_section_tag_supported() {
    let html = r#"<section><a href="/x">Short</a><a href="/y">Short2</a></section><section><p>This section contains the primary article content with enough text to be identified by the readability scoring algorithm as the main content block on the page, exceeding the navigation section in text density.</p></section>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("primary article content"));
    assert!(!md.contains("Short2"));
}

#[test]
fn paragraph_readability_extracts_dense_cluster() {
    let html = r#"<div><a href="/">Home</a><a href="/about">About</a></div><p>Short nav text.</p><p>This is the first paragraph of the main article content with substantial text that should be captured by the paragraph-level readability scoring algorithm as part of a dense cluster.</p><p>Here is the second paragraph continuing the article with more meaningful content that contributes to the overall text density of this cluster of paragraphs.</p><p>The third paragraph adds even more substantial text content to ensure the sliding window picks up this cluster as the highest scoring region of the page.</p><p>Finally the fourth paragraph rounds out the content cluster with additional text that should push the combined score well above the threshold for extraction.</p><div><a href="/privacy">Privacy</a><a href="/terms">Terms</a></div>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("first paragraph"));
    assert!(md.contains("fourth paragraph"));
    assert!(!md.contains("Privacy"));
    assert!(!md.contains("Terms"));
}

#[test]
fn paragraph_readability_falls_back_when_no_divs() {
    let html = r#"<p>Brief intro.</p><p>This is a substantial article paragraph with enough text content to be identified as the main content by the paragraph-level readability scoring algorithm when no div or section containers are present.</p><p>Another paragraph with additional content to build up the text density score of this cluster for extraction by the sliding window approach.</p><p>More content here to ensure the window score exceeds the threshold for meaningful extraction by the algorithm.</p>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("substantial article paragraph"));
}

#[test]
fn paragraph_readability_skips_short_paragraphs() {
    let html = r#"<p>OK.</p><p>Sure.</p><p>Yep.</p><p>No.</p><p>Fine.</p>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    // All paragraphs are too short — should return full HTML
    assert!(md.contains("OK") || md.contains("Sure") || md.contains("Yep"));
}

#[test]
fn paragraph_readability_extracts_best_window() {
    let html = r#"<p>Nav link one.</p><p>Nav link two.</p><p>Nav link three.</p><p>This paragraph contains the real article content that should be extracted by the paragraph readability algorithm because it has substantially more text than the navigation paragraphs above and below it.</p><p>Continuing the real article content with another substantial paragraph that should be part of the extracted window along with the previous paragraph.</p><p>Footer text one.</p><p>Footer text two.</p>"#;
    let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
    assert!(md.contains("real article content"));
}

#[test]
fn comments_extracted_from_forum_page() {
    let html = r#"<html><head><title>Forum Thread</title></head><body>
        <h1>Discussion Topic</h1>
        <p>Original post content here.</p>
        <div class="comment" id="comment-1">
            <span class="author">Alice</span>
            <div class="comment-body"><p>First comment with some text.</p></div>
        </div>
        <div class="comment" id="comment-2">
            <span class="author">Bob</span>
            <div class="comment-body"><p>Second comment agreeing with Alice.</p></div>
        </div>
        <div class="comment" id="comment-3">
            <span class="author">Charlie</span>
            <div class="comment-body"><p>Third comment with a different perspective on the topic.</p></div>
        </div>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("## Comments"));
    assert!(md.contains("Alice"));
    assert!(md.contains("Bob"));
    assert!(md.contains("Charlie"));
    assert!(md.contains("First comment"));
    assert!(md.contains("Second comment"));
    assert!(md.contains("Third comment"));
}

#[test]
fn comments_not_extracted_from_non_forum_page() {
    let html = r#"<html><head><title>Article</title></head><body>
        <h1>Regular Article</h1>
        <p>This is a normal article with no comments section.</p>
        <p>It has multiple paragraphs of content.</p>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("## Comments"));
}

#[test]
fn extraction_quality_empty_markdown_is_zero() {
    assert_eq!(PageToMarkdown::extraction_quality("<html></html>", ""), 0.0);
}

#[test]
fn extraction_quality_rewards_article_content() {
    let html = r#"<html><head><title>T</title><meta name="author" content="A">
        <meta property="article:published_time" content="2026-01-01"></head>
        <body><article><h1>Title</h1><p>Long enough prose for a confident extraction quality score with headings and metadata present on the page.</p></article></body></html>"#;
    let md = "# Title\n\nLong enough prose for a confident extraction quality score with headings and metadata present on the page.";
    let q = PageToMarkdown::extraction_quality(html, md);
    assert!(q >= 0.6, "got {q}");
}

#[test]
fn no_comments_option_skips_forum_comments() {
    let html = r#"<html><body>
        <h1>Discussion Topic</h1>
        <p>Original post content here with enough text for the body.</p>
        <div class="comment" id="comment-1">
            <span class="author">Alice</span>
            <div class="comment-body"><p>First comment with some text.</p></div>
        </div>
        <div class="comment" id="comment-2">
            <span class="author">Bob</span>
            <div class="comment-body"><p>Second comment agreeing with Alice.</p></div>
        </div>
        <div class="comment" id="comment-3">
            <span class="author">Charlie</span>
            <div class="comment-body"><p>Third comment with a different perspective on the topic.</p></div>
        </div>
    </body></html>"#;
    let with_comments = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(with_comments.contains("## Comments"));
    let opts = ConvertOptions {
        include_comments: false,
        ..Default::default()
    };
    let without = PageToMarkdown::convert_with(html, &opts, &[]).unwrap();
    assert!(!without.contains("## Comments"));
    assert!(without.contains("Original post"));
}

#[test]
fn favor_precision_drops_sidebar_noise() {
    let html = r#"<div class="sidebar"><p>Short promo.</p><p>Buy now!</p></div>
        <article><p>This is the real article body with enough substantive prose that precision mode should keep it while discarding short sidebar blurbs around the page chrome.</p>
        <p>A second paragraph reinforces the main content density so the article candidate wins under a stricter score threshold.</p></article>"#;
    let opts = ConvertOptions {
        favor_precision: true,
        ..Default::default()
    };
    let md = PageToMarkdown::convert_with(html, &opts, &[]).unwrap();
    assert!(md.contains("real article body"));
    assert!(!md.contains("Short promo"));
    assert!(!md.contains("Buy now"));
}

#[test]
fn detect_page_type_article_forum_product() {
    assert_eq!(
        PageToMarkdown::detect_page_type("<html><body><article><p>x</p></article></body></html>"),
        "article"
    );
    assert_eq!(
        PageToMarkdown::detect_page_type(
            r#"<html><body><div class="comment"></div><div class="comment-body"></div></body></html>"#
        ),
        "forum"
    );
    assert_eq!(
        PageToMarkdown::detect_page_type(
            r#"<html><head><script type="application/ld+json">{"@type":"Product","name":"Widget"}</script></head><body></body></html>"#
        ),
        "product"
    );
    assert_eq!(
        PageToMarkdown::detect_page_type("<html><body><p>Hello</p></body></html>"),
        "page"
    );
}

#[test]
fn article_profile_prefers_main_content_without_flag() {
    let html = r#"<div class="sidebar"><p>Sidebar promo that should be dropped by the article profile.</p></div>
        <article><p>This is the real article body that the page-type profile should keep via main-content extraction.</p></article>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("real article body"));
    assert!(!md.contains("Sidebar promo"));
}

#[test]
fn product_profile_appends_json_ld_details() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"Product","name":"Acme Widget","brand":{"@type":"Brand","name":"Acme"},
         "sku":"W-100","offers":{"@type":"Offer","price":"19.99","priceCurrency":"USD"}}
        </script>
        </head><body><main><p>Buy the Acme Widget today with free shipping on orders over fifty dollars.</p>
        <img src="/widget.png" alt="Widget"></main></body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("## Product details"));
    assert!(md.contains("Acme Widget"));
    assert!(md.contains("Acme"));
    assert!(md.contains("W-100"));
    assert!(md.contains("19.99 USD"));
    assert!(md.contains("![Widget](/widget.png)") || md.contains("/widget.png"));
}

#[test]
fn comments_extracted_with_data_author() {
    let html = r#"<html><body>
        <h1>Thread</h1>
        <p>Post content.</p>
        <div class="comment" data-author="johndoe">
            <div class="comment-body"><p>Great post, thanks for sharing.</p></div>
        </div>
        <div class="comment" data-author="janedoe">
            <div class="comment-body"><p>I disagree with some points.</p></div>
        </div>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("## Comments"));
    assert!(md.contains("johndoe"));
    assert!(md.contains("janedoe"));
    assert!(md.contains("Great post"));
}

#[test]
fn comments_not_extracted_with_only_one_comment() {
    let html = r#"<html><body>
        <h1>Page</h1>
        <p>Content.</p>
        <div class="comment" id="comment-1">
            <span class="author">Solo</span>
            <div class="comment-body"><p>Only one comment here.</p></div>
        </div>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(!md.contains("## Comments"));
}

#[test]
fn comments_nested_with_indentation() {
    let html = r#"<html><body>
        <h1>Thread</h1>
        <p>Post.</p>
        <div class="comment" id="comment-1">
            <span class="author">Parent</span>
            <div class="comment-body"><p>Top level comment.</p></div>
            <div class="comment" id="comment-2">
                <span class="author">Child</span>
                <div class="comment-body"><p>Reply to parent.</p></div>
            </div>
        </div>
        <div class="comment" id="comment-3">
            <span class="author">Another</span>
            <div class="comment-body"><p>Another top level comment.</p></div>
        </div>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("## Comments"));
    assert!(md.contains("Parent"));
    assert!(md.contains("Child"));
    assert!(md.contains("Top level comment"));
    assert!(md.contains("Reply to parent"));
}

#[test]
fn comments_extracted_from_reddit_style() {
    let html = r#"<html><body>
        <h1>Reddit Post</h1>
        <p>Post content.</p>
        <div class="comment" data-testid="comment-1">
            <div data-author="redditor1">
                <div class="md"><p>Reddit style comment.</p></div>
            </div>
        </div>
        <div class="comment" data-testid="comment-2">
            <div data-author="redditor2">
                <div class="md"><p>Another Reddit comment.</p></div>
            </div>
        </div>
    </body></html>"#;
    let md = PageToMarkdown::convert(html, false, false, false, &[]).unwrap();
    assert!(md.contains("## Comments"));
    assert!(md.contains("redditor1"));
    assert!(md.contains("Reddit style comment"));
}

#[test]
fn absolutize_links_converts_relative_to_absolute() {
    let md = "[About](/about) [Contact](/contact-us)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
    assert!(result.contains("https://example.com/about"));
    assert!(result.contains("https://example.com/contact-us"));
}

#[test]
fn absolutize_links_handles_protocol_relative() {
    let md = "[Link](//cdn.example.com/file)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
    assert!(result.contains("https://cdn.example.com/file"));
}

#[test]
fn absolutize_links_leaves_absolute_unchanged() {
    let md = "[Link](https://other.com/page)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
    assert!(result.contains("https://other.com/page"));
}

#[test]
fn absolutize_links_leaves_anchor_links_unchanged() {
    let md = "[Section](#section)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
    assert!(result.contains("#section"));
}

#[test]
fn absolutize_links_resolves_relative_paths() {
    let md = "[Prev](../parent) [Next](./child)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/blog/post");
    // ../parent from /blog/post → /parent (go up from post to blog, then up from blog to root)
    assert!(result.contains("https://example.com/parent"), "got: {}", result);
    // ./child from /blog/post → /blog/child (same directory as post)
    assert!(result.contains("https://example.com/blog/child"), "got: {}", result);
}

#[test]
fn absolutize_links_handles_image_links() {
    let md = "![Photo](/images/photo.jpg)";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com/page");
    assert!(result.contains("https://example.com/images/photo.jpg"));
}

#[test]
fn absolutize_links_preserves_surrounding_text() {
    let md = "Hello [world](/world) and [universe](/universe) end";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com");
    assert!(result.starts_with("Hello "));
    assert!(result.contains("https://example.com/world"));
    assert!(result.contains("https://example.com/universe"));
    assert!(result.ends_with("end"));
}

#[test]
fn absolutize_links_invalid_base_returns_original() {
    let md = "[Link](/path)";
    let result = PageToMarkdown::absolutize_links(md, "not-a-url");
    assert_eq!(result, md);
}

#[test]
fn absolutize_links_preserves_unicode_text() {
    let md = "[リンク](/page) 日本語テキスト";
    let result = PageToMarkdown::absolutize_links(md, "https://example.com");
    assert!(result.contains("https://example.com/page"));
    assert!(result.contains("日本語テキスト"));
}

#[test]
fn exclude_selector_class_removes_element() {
    let html = r#"<p>Keep this</p><div class="ad">Advertisement</div><p>Also keep</p>"#;
    let selectors = vec![".ad".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep this"));
    assert!(md.contains("Also keep"));
    assert!(!md.contains("Advertisement"));
}

#[test]
fn exclude_selector_id_removes_element() {
    let html = r#"<p>Keep this</p><div id="sidebar">Sidebar content</div><p>Also keep</p>"#;
    let selectors = vec!["#sidebar".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep this"));
    assert!(md.contains("Also keep"));
    assert!(!md.contains("Sidebar"));
}

#[test]
fn exclude_selector_multiple_selectors() {
    let html = r#"<div class="ad">Ad</div><p>Keep</p><div id="promo">Promo</div><p>Also keep</p>"#;
    let selectors = vec![".ad".to_string(), "#promo".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep"));
    assert!(md.contains("Also keep"));
    assert!(!md.contains("Ad"));
    assert!(!md.contains("Promo"));
}

#[test]
fn exclude_selector_class_word_boundary() {
    let html = r#"<div class="ad-banner">Banner</div><p>Keep</p><div class="notad">Should keep</div>"#;
    let selectors = vec![".ad".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep"));
    assert!(md.contains("Banner"));
    assert!(md.contains("Should keep"));
}

#[test]
fn exclude_selector_class_multi_class_match() {
    let html = r#"<div class="box highlight">Highlighted</div><p>Keep</p>"#;
    let selectors = vec![".highlight".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep"));
    assert!(!md.contains("Highlighted"));
}

#[test]
fn exclude_selector_empty_does_nothing() {
    let html = r#"<p>Keep this</p><div class="ad">Ad</div>"#;
    let selectors: Vec<String> = vec![];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep this"));
    assert!(md.contains("Ad"));
}

#[test]
fn exclude_selector_nested_elements_removed() {
    let html = r#"<div class="ad"><h2>Ad Title</h2><p>Ad content</p></div><p>Keep this</p>"#;
    let selectors = vec![".ad".to_string()];
    let md = PageToMarkdown::convert(html, false, false, false, &selectors).unwrap();
    assert!(md.contains("Keep this"));
    assert!(!md.contains("Ad Title"));
    assert!(!md.contains("Ad content"));
}

#[test]
fn to_plain_text_strips_markdown_syntax() {
    let md = "# Title\n\nHello **world** with [a link](https://example.com).";
    let text = PageToMarkdown::to_plain_text(md);
    assert!(text.contains("Title"));
    assert!(text.contains("Hello world"));
    assert!(text.contains("a link"));
    assert!(!text.contains("**"));
    assert!(!text.contains("](https://"));
}

#[test]
fn to_plain_text_preserves_code_content() {
    let md = "Use `println!` in Rust.";
    let text = PageToMarkdown::to_plain_text(md);
    assert!(text.contains("println!"));
    assert!(!text.contains('`'));
}

#[test]
fn to_plain_text_strips_list_markers() {
    let md = "* one\n* two\n\nParagraph.";
    let text = PageToMarkdown::to_plain_text(md);
    assert!(text.contains("one"));
    assert!(text.contains("two"));
    assert!(text.contains("Paragraph"));
    assert!(!text.contains("* one"));
}

#[test]
fn inject_code_languages_only_into_opening_fence() {
    let md = "```\nfn main() {}\n```";
    let langs = vec!["rust".to_string()];
    let result = PageToMarkdown::inject_code_languages(md, &langs);
    let lines: Vec<&str> = result.lines().collect();
    assert!(lines[0].starts_with("```rust"));
    assert_eq!(lines[2], "```"); // closing fence has no language
}

#[test]
fn inject_code_languages_handles_multiple_blocks() {
    let md = "```\ncode1\n```\n\ntext\n\n```\ncode2\n```";
    let langs = vec!["rust".to_string(), "python".to_string()];
    let result = PageToMarkdown::inject_code_languages(md, &langs);
    let lines: Vec<&str> = result.lines().collect();
    assert!(lines[0].starts_with("```rust"));
    assert_eq!(lines[2], "```");
    assert!(lines[6].starts_with("```python"));
    assert_eq!(lines[8], "```");
}

#[test]
fn strip_boilerplate_removes_cookie_banner() {
    let html = r#"<div class="cookie-banner">We use cookies</div><p>Real content</p>"#;
    let result = PageToMarkdown::strip_boilerplate_elements(html);
    assert!(!result.contains("cookie"));
    assert!(result.contains("Real content"));
}

#[test]
fn strip_boilerplate_removes_social_share() {
    let html = r#"<div class="social-share"><a href="twitter">Tweet</a></div><p>Article text</p>"#;
    let result = PageToMarkdown::strip_boilerplate_elements(html);
    assert!(!result.contains("Tweet"));
    assert!(result.contains("Article text"));
}

#[test]
fn strip_boilerplate_removes_breadcrumb() {
    let html = r#"<nav class="breadcrumb"><a href="/">Home</a> > <a href="/cat">Category</a></nav><p>Content</p>"#;
    let result = PageToMarkdown::strip_boilerplate_elements(html);
    assert!(!result.contains("breadcrumb"));
}

#[test]
fn strip_boilerplate_preserves_content_divs() {
    let html = r#"<div class="article-content"><p>Important text</p></div>"#;
    let result = PageToMarkdown::strip_boilerplate_elements(html);
    assert!(result.contains("Important text"));
}

#[test]
fn strip_boilerplate_handles_nested_elements() {
    let html = r#"<div class="cookie-consent"><div><p>Cookie text</p></div></div><p>Real content</p>"#;
    let result = PageToMarkdown::strip_boilerplate_elements(html);
    assert!(!result.contains("Cookie text"));
    assert!(result.contains("Real content"));
}

#[test]
fn clean_markdown_noise_removes_wikipedia_citations() {
    let md = "Rust was created in 2006.^([[ 20 ]](#cite_note-MITTechReview-24))";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert!(!result.contains("cite_note"));
    assert!(!result.contains("[[ 20 ]]"));
    assert!(result.contains("Rust was created in 2006."));
}

#[test]
fn clean_markdown_noise_removes_stacked_citations() {
    let md = "Text.^([[ 20 ]](#cite_note-a))^([[ 21 ]](#cite_note-b))";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert!(!result.contains("cite_note"));
    assert!(result.contains("Text."));
}

#[test]
fn clean_markdown_noise_removes_edit_links() {
    let md = "History\n----------\n\n[ [edit](/w/index.php?title=Rust&action=edit) ]\n\nContent here.";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert!(!result.contains("edit"));
    assert!(result.contains("Content here."));
}

#[test]
fn clean_markdown_noise_removes_file_links() {
    let md = "[File:Rust_logo.svg](https://en.wikipedia.org/wiki/File:Rust_logo.svg)\n\nContent.";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert!(!result.contains("File:"));
    assert!(result.contains("Content."));
}

#[test]
fn clean_markdown_noise_unwraps_heading_anchor_links() {
    let md = "### [Command Line Notation](#command-line-notation) ###";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert_eq!(result, "### Command Line Notation ###");
}

#[test]
fn clean_markdown_noise_unwraps_setext_heading_anchor() {
    let md = "[Installation](#installation)\n==========\n\nBody text.";
    let result = PageToMarkdown::clean_markdown_noise(md);
    assert!(result.contains("Installation\n=========="));
    assert!(!result.contains("](#installation)"));
}
