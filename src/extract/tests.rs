use super::*;

#[test]
fn extract_links_resolves_relative_urls() {
    let html = r#"<a href="/about">About Us</a><a href="contact">Contact</a>"#;
    let links = extract_links(html, "https://example.com/blog/post");
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].url, "https://example.com/about");
    assert_eq!(links[0].text, "About Us");
    assert_eq!(links[1].url, "https://example.com/blog/contact");
    assert_eq!(links[1].text, "Contact");
}

#[test]
fn extract_links_skips_empty_and_fragment_hrefs() {
    let html = r##"<a href="#top">Top</a><a href="">Empty</a><a href="/ok">OK</a>"##;
    let links = extract_links(html, "https://example.com/");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://example.com/ok");
}

#[test]
fn extract_links_deduplicates() {
    let html = r#"<a href="/a">A</a><a href="/a">A again</a>"#;
    let links = extract_links(html, "https://example.com/");
    assert_eq!(links.len(), 1);
}

#[test]
fn extract_links_strips_inner_html_tags() {
    let html = r#"<a href="/x"><b>Bold</b> Link</a>"#;
    let links = extract_links(html, "https://example.com/");
    assert_eq!(links[0].text, "Bold Link");
}

#[test]
fn extract_images_resolves_src() {
    let html = r#"<img src="/img/logo.png" alt="Logo"><img src="https://cdn.com/x.jpg">"#;
    let images = extract_images(html, "https://example.com/page");
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].src, "https://example.com/img/logo.png");
    assert_eq!(images[0].alt.as_deref(), Some("Logo"));
    assert_eq!(images[1].src, "https://cdn.com/x.jpg");
    assert!(images[1].alt.is_none());
}

#[test]
fn extract_images_deduplicates() {
    let html = r#"<img src="/a.png"><img src="/a.png" alt="dup">"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
}

#[test]
fn extract_images_skips_data_urls() {
    let html = r#"<img src="data:image/png;base64,abc"><img src="/real.png">"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].src, "https://example.com/real.png");
}

#[test]
fn extract_images_from_background_css() {
    let html = r#"<div style="background-image: url('/bg.jpg')">Hello</div>"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].src, "https://example.com/bg.jpg");
}

#[test]
fn extract_images_from_background_css_quoted() {
    let html = r#"<div style="background: url('/img/hero.png') no-repeat">Hero</div>"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].src, "https://example.com/img/hero.png");
}

#[test]
fn extract_images_from_single_quoted_style() {
    let html = r#"<div style='background-image: url("/hero.jpg")'>Hero</div>"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].src, "https://example.com/hero.jpg");
}

#[test]
fn extract_images_from_style_block() {
    let html = r#"<style>.hero { background-image: url('/css-bg.png'); }</style><p>Hi</p>"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].src, "https://example.com/css-bg.png");
}

#[test]
fn extract_images_dedup_across_img_and_bg() {
    let html = r#"<img src="/shared.png"><div style="background-image: url('/shared.png')">BG</div>"#;
    let images = extract_images(html, "https://example.com/");
    assert_eq!(images.len(), 1);
}

#[test]
fn extract_product_from_json_ld() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"Product","name":"Acme Widget","brand":{"@type":"Brand","name":"Acme"},
         "sku":"W-100","description":"A great widget","category":"Tools",
         "offers":{"@type":"Offer","price":"19.99","priceCurrency":"USD",
                   "availability":"https://schema.org/InStock"}}
        </script>
        </head><body></body></html>"#;
    let product = extract_product(html).unwrap();
    assert_eq!(product.name.as_deref(), Some("Acme Widget"));
    assert_eq!(product.brand.as_deref(), Some("Acme"));
    assert_eq!(product.sku.as_deref(), Some("W-100"));
    assert_eq!(product.description.as_deref(), Some("A great widget"));
    assert_eq!(product.category.as_deref(), Some("Tools"));
    assert_eq!(product.variants.len(), 1);
    assert_eq!(product.variants[0].price.as_deref(), Some("19.99"));
    assert_eq!(product.variants[0].currency.as_deref(), Some("USD"));
}

#[test]
fn extract_product_returns_none_without_json_ld() {
    let html = "<html><body><p>No product here</p></body></html>";
    assert!(extract_product(html).is_none());
}

#[test]
fn extract_product_handles_multiple_offers() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"Product","name":"Widget",
         "offers":[{"@type":"Offer","price":"10.00","priceCurrency":"USD"},
                    {"@type":"Offer","price":"8.50","priceCurrency":"EUR"}]}
        </script></head><body></body></html>"#;
    let product = extract_product(html).unwrap();
    assert_eq!(product.variants.len(), 2);
    assert_eq!(product.variants[0].price.as_deref(), Some("10.00"));
    assert_eq!(product.variants[1].price.as_deref(), Some("8.50"));
}

#[test]
fn extract_product_handles_image_as_array() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"Product","name":"Widget","image":["https://x.com/a.jpg","https://x.com/b.jpg"]}
        </script></head><body></body></html>"#;
    let product = extract_product(html).unwrap();
    assert_eq!(product.image.as_deref(), Some("https://x.com/a.jpg"));
}

#[test]
fn extract_videos_from_video_src() {
    let html = r#"<video src="https://example.com/video.mp4" poster="https://example.com/poster.jpg" title="Demo" duration="PT1M30S"></video>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].url, "https://example.com/video.mp4");
    assert_eq!(videos[0].source.as_deref(), Some("video"));
    assert_eq!(videos[0].thumbnail.as_deref(), Some("https://example.com/poster.jpg"));
    assert_eq!(videos[0].title.as_deref(), Some("Demo"));
    assert_eq!(videos[0].duration, Some(90.0));
}

#[test]
fn extract_videos_from_source_tags() {
    let html = r#"<video><source src="https://example.com/video.webm" type="video/webm"><source src="https://example.com/video.mp4" type="video/mp4"></video>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 2);
    assert_eq!(videos[0].url, "https://example.com/video.webm");
    assert_eq!(videos[0].source.as_deref(), Some("source"));
    assert_eq!(videos[1].url, "https://example.com/video.mp4");
}

#[test]
fn extract_videos_from_iframe_embeds() {
    let html = r#"<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].source.as_deref(), Some("youtube"));
}

#[test]
fn extract_videos_skips_non_video_iframes() {
    let html = r#"<iframe src="https://example.com/ads.html"></iframe>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 0);
}

#[test]
fn extract_videos_deduplicates() {
    let html = r#"<video src="https://example.com/video.mp4"></video><video src="https://example.com/video.mp4"></video>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 1);
}

#[test]
fn extract_videos_from_json_ld_video_object() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"VideoObject","name":"Talk","contentUrl":"https://cdn.example.com/talk.mp4",
         "thumbnailUrl":"https://cdn.example.com/talk.jpg","duration":"PT2M"}
        </script></head><body></body></html>"#;
    let videos = extract_videos(html, "https://example.com/");
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].title.as_deref(), Some("Talk"));
    assert_eq!(videos[0].thumbnail.as_deref(), Some("https://cdn.example.com/talk.jpg"));
    assert_eq!(videos[0].duration, Some(120.0));
    assert_eq!(videos[0].source.as_deref(), Some("json-ld"));
}

#[test]
fn extract_audios_from_audio_src() {
    let html = r#"<audio src="https://example.com/podcast.mp3" title="Episode 1" duration="PT45M"></audio>"#;
    let audios = extract_audios(html, "https://example.com/");
    assert_eq!(audios.len(), 1);
    assert_eq!(audios[0].url, "https://example.com/podcast.mp3");
    assert_eq!(audios[0].source.as_deref(), Some("audio"));
    assert_eq!(audios[0].title.as_deref(), Some("Episode 1"));
    assert_eq!(audios[0].duration, Some(2700.0));
}

#[test]
fn extract_audios_from_source_tags() {
    let html = r#"<audio><source src="https://example.com/podcast.ogg" type="audio/ogg"><source src="https://example.com/podcast.mp3" type="audio/mpeg"></audio>"#;
    let audios = extract_audios(html, "https://example.com/");
    assert_eq!(audios.len(), 2);
    assert_eq!(audios[0].url, "https://example.com/podcast.ogg");
    assert_eq!(audios[0].source.as_deref(), Some("source"));
    assert_eq!(audios[1].url, "https://example.com/podcast.mp3");
}

#[test]
fn extract_audios_from_iframe_embeds() {
    let html = r#"<iframe src="https://open.spotify.com/embed/episode/abc123" title="Spotify Episode"></iframe>"#;
    let audios = extract_audios(html, "https://example.com/");
    assert_eq!(audios.len(), 1);
    assert_eq!(audios[0].source.as_deref(), Some("spotify"));
    assert_eq!(audios[0].title.as_deref(), Some("Spotify Episode"));
}

#[test]
fn extract_audios_deduplicates() {
    let html = r#"<audio src="https://example.com/podcast.mp3"></audio><audio src="https://example.com/podcast.mp3"></audio>"#;
    let audios = extract_audios(html, "https://example.com/");
    assert_eq!(audios.len(), 1);
}

#[test]
fn extract_audios_from_json_ld_audio_object() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"AudioObject","name":"Interview","contentUrl":"https://cdn.example.com/interview.mp3","duration":"PT30M"}
        </script></head><body></body></html>"#;
    let audios = extract_audios(html, "https://example.com/");
    assert_eq!(audios.len(), 1);
    assert_eq!(audios[0].title.as_deref(), Some("Interview"));
    assert_eq!(audios[0].duration, Some(1800.0));
    assert_eq!(audios[0].source.as_deref(), Some("json-ld"));
}

#[test]
fn extract_attributes_collects_values() {
    let html = r#"<a href="/a">A</a><a href="/b">B</a><img src="/x.png" data-id="1">"#;
    let specs = vec!["a:href".into(), "img:data-id".into()];
    let results = extract_attributes(html, &specs);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].values, vec!["/a", "/b"]);
    assert_eq!(results[1].values, vec!["1"]);
}

#[test]
fn extract_menu_from_json_ld() {
    let html = r#"<html><head>
        <script type="application/ld+json">
        {"@type":"Menu","name":"Dinner",
         "hasMenuSection":[{
           "@type":"MenuSection","name":"Mains",
           "hasMenuItem":[{
             "@type":"MenuItem","name":"Pasta",
             "description":"Tomato sauce",
             "offers":{"@type":"Offer","price":"14.50","priceCurrency":"USD"}
           }]
         }]}
        </script></head><body></body></html>"#;
    let menu = extract_menu(html).unwrap();
    assert_eq!(menu.name.as_deref(), Some("Dinner"));
    assert_eq!(menu.currency.as_deref(), Some("USD"));
    assert_eq!(menu.sections.len(), 1);
    assert_eq!(menu.sections[0].name.as_deref(), Some("Mains"));
    assert_eq!(menu.sections[0].items[0].name.as_deref(), Some("Pasta"));
    assert_eq!(menu.sections[0].items[0].price.as_deref(), Some("14.50"));
}

#[test]
fn extract_menu_returns_none_without_json_ld() {
    assert!(extract_menu("<html><body>No menu</body></html>").is_none());
}
