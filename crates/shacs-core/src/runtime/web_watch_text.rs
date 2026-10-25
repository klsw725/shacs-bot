use std::sync::OnceLock;

use regex::{Captures, Regex};
use unicode_normalization::UnicodeNormalization;

/// Elements whose tags become one space so neighbouring blocks do not run together.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "option",
    "p",
    "pre",
    "section",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];

static MARKUP: OnceLock<Option<Regex>> = OnceLock::new();

/// `normalized_static_text_v1`: comments and `script`/`style` elements removed,
/// block tags replaced by a space and other tags removed, character references
/// decoded, Unicode NFC applied and whitespace collapsed to single spaces.
pub(super) fn normalized_static_text_v1(html: &str) -> String {
    // One left-to-right pass, as an HTML tokenizer reads: a tag name ends at whitespace, `/`
    // or `>` (so `<script-widget>` is not a script), and a quoted attribute value (a quote
    // right after `=`) may hold `>`, `<!--` or `</script>` without ending or opening anything.
    let stripped = regex(
        &MARKUP,
        concat!(
            r#"(?is)<!--.*?(?:-->|\z)"#,
            r#"|<script(?:[\s/](?:=\s*"[^"]*"|=\s*'[^']*'|[^>])*)?>.*?(?:</script(?:[\s/][^>]*)?>|\z)"#,
            r#"|<style(?:[\s/](?:=\s*"[^"]*"|=\s*'[^']*'|[^>])*)?>.*?(?:</style(?:[\s/][^>]*)?>|\z)"#,
            r#"|</?([A-Za-z][^\s/>]*)(?:[\s/](?:=\s*"[^"]*"|=\s*'[^']*'|[^>])*)?>"#,
            r#"|<[!?][^>]*>"#,
        ),
    )
    .map_or_else(
        || html.to_owned(),
        |regex| {
            regex
                .replace_all(html, |captures: &Captures<'_>| {
                    let block = captures.get(1).is_some_and(|name| {
                        BLOCK_TAGS.contains(&name.as_str().to_ascii_lowercase().as_str())
                    });
                    if block {
                        " "
                    } else {
                        ""
                    }
                })
                .into_owned()
        },
    );
    normalize_fragment(&html_escape::decode_html_entities(&stripped))
}

/// NFC and whitespace collapsing only, for user-supplied labels and phrases.
pub(super) fn normalize_fragment(text: &str) -> String {
    let composed: String = text.nfc().collect();
    let mut collapsed = String::with_capacity(composed.len());
    for word in composed
        .split(char::is_whitespace)
        .filter(|word| !word.is_empty())
    {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    collapsed
}

fn regex(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

#[cfg(test)]
mod tests {
    use super::normalized_static_text_v1;

    #[test]
    fn v1_drops_scripts_styles_and_comments_and_decodes_after_tag_removal() {
        let html = "<html><head><title>Shop</title><style>p{}</style>\
            <script>var price = 1;</script></head><body><!-- hidden 9 -->\
            <div>Price:<b>&#8361;12,900</b></div><p>A&nbsp;&nbsp; &lt;b&gt;  &euro;\n\tB</p>\
            <script>unclosed 7";

        let text = normalized_static_text_v1(html);

        assert_eq!(text, "Shop Price:\u{20a9}12,900 A <b> \u{20ac} B");
    }

    #[test]
    fn v1_removes_a_tag_whose_quoted_attribute_holds_a_closing_bracket() {
        let html = "<p data-note=\"x > 판매가 &#8361;1 배송비\" title='a > b'>판매가 &#8361;12,900 배송비</p>\
            <a href=x'y>link</a>";

        let text = normalized_static_text_v1(html);

        assert_eq!(text, "판매가 \u{20a9}12,900 배송비 link");
    }

    #[test]
    fn v1_reads_left_to_right_so_attribute_values_open_or_close_nothing() {
        let html = "<script data-x=\"x > </script>\">판매가 &#8361;1 배송비</script>\
            <div title=\"<!-- note\">판매가 &#8361;12,900 배송비</div><p title='<script>'>무료</p>";

        let text = normalized_static_text_v1(html);

        assert_eq!(text, "판매가 \u{20a9}12,900 배송비 무료");
    }

    #[test]
    fn v1_keeps_custom_elements_whose_names_start_like_script_style_or_div() {
        let html = "<script-widget>판매가 &#8361;12,900</script-widget> <style-guide>배송비</style-guide> \
            a<div-card>b</div-card>c<script >var x = 1;</script ><p>끝</p>";

        let text = normalized_static_text_v1(html);

        assert_eq!(text, "판매가 \u{20a9}12,900 배송비 abc 끝");
    }

    #[test]
    fn v1_composes_unicode_to_nfc() {
        let text = normalized_static_text_v1("<p>\u{1100}\u{1161}\u{11a8} cafe\u{301}</p>");

        assert_eq!(text, "\u{ac01} caf\u{e9}");
    }
}
