//! The few XML reads the location formats need, with the rules the C#
//! plugin's `XmlDocument` had: element and attribute names match exactly,
//! a child is the first child element of that name, and an element's text
//! is all the text inside it with the white-space-only pieces between
//! elements left out. A document type declaration is refused.

use roxmltree::{Document, Node, ParsingOptions};

/// Parses a document. A leading byte-order mark is skipped; a DTD is an
/// error (no entity expansion from downloaded or hand-edited files).
pub(crate) fn parse(xml: &str) -> Result<Document<'_>, String> {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let options = ParsingOptions {
        allow_dtd: false,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(xml, options).map_err(|e| e.to_string())
}

/// Whether the node is an element with this name and no namespace.
pub(crate) fn is_named(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace().is_none() && node.tag_name().name() == name
}

/// The first child element with this name.
pub(crate) fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|c| is_named(*c, name))
}

/// The text of the first child element with this name.
pub(crate) fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    child(node, name).map(inner_text)
}

/// All text inside the node, in document order, without the
/// white-space-only text between elements.
pub(crate) fn inner_text(node: Node<'_, '_>) -> String {
    let mut out = String::new();
    for d in node.descendants() {
        if let Some(text) = d.is_text().then(|| d.text()).flatten()
            && !is_xml_white(text)
        {
            out.push_str(text);
        }
    }
    out
}

/// The node's own text children (not those of its child elements), each
/// one that is not white space only.
pub(crate) fn own_texts<'a>(node: Node<'a, '_>) -> impl Iterator<Item = &'a str> {
    node.children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .filter(|t| !is_xml_white(t))
}

fn is_xml_white(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

/// Escapes text for element content.
pub(crate) fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// Escapes text for a double-quoted attribute value; line breaks and tabs
/// as character references, so they read back as written.
pub(crate) fn escape_attribute(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            _ => out.push(c),
        }
    }
}
