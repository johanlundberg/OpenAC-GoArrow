//! Reading a location file in bounded steps, so a 5 MB Atlas export does
//! not hold the game's session thread for one long call.
//!
//! The document is cut into the root element's children; each child is
//! parsed on its own and its records read, one child per unit of work.
//! What is left (the prolog, the root's tags, the text and comments
//! between children, the epilog) is parsed last as a skeleton, so the
//! whole file is checked as strictly as one parse would check it. A file
//! that does not split cleanly (a namespace on the root, a record as the
//! root, a declaration, anything malformed) is parsed in one piece
//! instead, which gives the same result and the parser's own error, when
//! it is at most [WHOLE_DOCUMENT_LIMIT]; a larger one is refused
//! ([LoadError::Unsplit], [AtlasError::Unsplit]) rather than parsed in
//! one long call.
//!
//! The result is what [`crate::database::parse_locations_xml`] and
//! [`crate::database::validate_atlas_xml`] give.

use roxmltree::Document;

use crate::database::{ATLAS_MAX_BYTES, AtlasError, LoadError, ParsedLocations};
use crate::location::{Location, LocationError};
use crate::xml;

/// The largest file parsed in one piece when it does not split into its
/// records, bytes (1 MB).
pub const WHOLE_DOCUMENT_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Prolog,
    Content,
    Epilog,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Markup {
    Comment,
    CData,
    Instruction,
    Declaration,
    EndTag,
    StartTag { self_closing: bool },
}

/// The markup that starts at `at` (a `<`) and where it ends.
fn markup_at(s: &str, at: usize) -> Option<(Markup, usize)> {
    let rest = &s[at..];
    let find = |from: usize, needle: &str| s[from..].find(needle).map(|i| from + i + needle.len());
    if rest.starts_with("<!--") {
        find(at + 4, "-->").map(|end| (Markup::Comment, end))
    } else if rest.starts_with("<![CDATA[") {
        find(at + 9, "]]>").map(|end| (Markup::CData, end))
    } else if rest.starts_with("<?") {
        find(at + 2, "?>").map(|end| (Markup::Instruction, end))
    } else if rest.starts_with("<!") {
        Some((Markup::Declaration, at))
    } else if rest.starts_with("</") {
        find(at + 2, ">").map(|end| (Markup::EndTag, end))
    } else {
        let bytes = s.as_bytes();
        let mut quote = None;
        for (i, &c) in bytes.iter().enumerate().skip(at + 1) {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None => match c {
                    b'"' | b'\'' => quote = Some(c),
                    b'>' => {
                        let self_closing = bytes[i - 1] == b'/';
                        return Some((Markup::StartTag { self_closing }, i + 1));
                    }
                    _ => {}
                },
            }
        }
        None
    }
}

/// Where the element whose start tag ends at `from` ends.
fn element_end(s: &str, mut from: usize) -> Option<usize> {
    let mut depth = 1usize;
    loop {
        let at = from + s[from..].find('<')?;
        let (markup, end) = markup_at(s, at)?;
        match markup {
            Markup::Comment | Markup::CData | Markup::Instruction => {}
            Markup::Declaration => return None,
            Markup::EndTag => {
                depth -= 1;
                if depth == 0 {
                    return Some(end);
                }
            }
            Markup::StartTag { self_closing } => {
                if !self_closing {
                    depth += 1;
                }
            }
        }
        from = end;
    }
}

fn tag_name(tag: &str) -> &str {
    let body = tag.trim_start_matches('<');
    let end = body
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(body.len());
    &body[..end]
}

/// The records of a document read so far.
#[derive(Debug)]
struct Scanner {
    text: String,
    pos: usize,
    stage: Stage,
    skeleton: String,
    /// Read the records (a location file, or an Atlas check whose root is
    /// `<atlas>`).
    read_records: bool,
    /// Atlas mode: only an `<atlas>` root's records are read.
    atlas_check: bool,
    is_atlas: bool,
    root_name: String,
    atlas_children: usize,
    locations: Vec<Location>,
    lines: Vec<String>,
    record_error: Option<LocationError>,
    xml_error: Option<String>,
    /// Too large to parse in one piece, and it did not split.
    unsplit: bool,
}

impl Scanner {
    fn new(text: String, atlas_check: bool) -> Self {
        let start = if text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        Self {
            text,
            pos: start,
            stage: Stage::Prolog,
            skeleton: String::new(),
            read_records: !atlas_check,
            atlas_check,
            is_atlas: false,
            root_name: String::new(),
            atlas_children: 0,
            locations: Vec::new(),
            lines: Vec::new(),
            record_error: None,
            xml_error: None,
            unsplit: false,
        }
    }

    /// Does at most `budget` units of work; true once finished.
    fn step(&mut self, budget: &mut usize) -> bool {
        while *budget > 0 {
            match self.stage {
                Stage::Prolog => self.prolog(),
                Stage::Content => self.content(budget),
                Stage::Epilog => {
                    self.skeleton.push_str(&self.text[self.pos..]);
                    *budget -= 1;
                    let skeleton = std::mem::take(&mut self.skeleton);
                    self.stage = Stage::Finished;
                    if xml::parse(&skeleton).is_err() {
                        self.whole_document();
                    }
                }
                Stage::Finished => return true,
            }
        }
        self.stage == Stage::Finished
    }

    /// Up to and including the root's start tag.
    fn prolog(&mut self) {
        loop {
            let rest = &self.text[self.pos..];
            let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
            let skipped = rest.len() - trimmed.len();
            self.skeleton.push_str(&rest[..skipped]);
            self.pos += skipped;
            if !trimmed.starts_with('<') {
                return self.whole_document();
            }
            let Some((markup, end)) = markup_at(&self.text, self.pos) else {
                return self.whole_document();
            };
            match markup {
                Markup::Comment | Markup::Instruction => {
                    self.skeleton.push_str(&self.text[self.pos..end]);
                    self.pos = end;
                }
                Markup::StartTag { self_closing } => {
                    let tag = &self.text[self.pos..end];
                    let name = tag_name(tag);
                    if tag.contains("xmlns")
                        || name.contains(':')
                        || name.eq_ignore_ascii_case("loc")
                        || name == "Location"
                    {
                        return self.whole_document();
                    }
                    self.root_name = name.to_owned();
                    self.is_atlas = name.eq_ignore_ascii_case("atlas");
                    if self.atlas_check {
                        self.read_records = self.is_atlas;
                    }
                    self.skeleton.push_str(tag);
                    self.pos = end;
                    self.stage = if self_closing {
                        Stage::Epilog
                    } else {
                        Stage::Content
                    };
                    return;
                }
                _ => return self.whole_document(),
            }
        }
    }

    /// The root's content, one child element per unit of work.
    fn content(&mut self, budget: &mut usize) {
        while *budget > 0 {
            let Some(offset) = self.text[self.pos..].find('<') else {
                return self.whole_document();
            };
            let at = self.pos + offset;
            self.skeleton.push_str(&self.text[self.pos..at]);
            let Some((markup, end)) = markup_at(&self.text, at) else {
                return self.whole_document();
            };
            match markup {
                Markup::Comment | Markup::CData | Markup::Instruction => {
                    self.skeleton.push_str(&self.text[at..end]);
                    self.pos = end;
                }
                Markup::Declaration => return self.whole_document(),
                Markup::EndTag => {
                    self.skeleton.push_str(&self.text[at..end]);
                    self.pos = end;
                    self.stage = Stage::Epilog;
                    return;
                }
                Markup::StartTag { self_closing } => {
                    let end = if self_closing {
                        Some(end)
                    } else {
                        element_end(&self.text, end)
                    };
                    let Some(end) = end else {
                        return self.whole_document();
                    };
                    *budget -= 1;
                    let text = std::mem::take(&mut self.text);
                    let parsed = xml::parse(&text[at..end]).map(|doc| self.read(&doc, true));
                    self.text = text;
                    if parsed.is_err() {
                        return self.whole_document();
                    }
                    self.pos = end;
                }
            }
        }
    }

    /// Parses the whole text at once: for anything the split does not
    /// handle, and to give a malformed file the parser's own error.
    fn whole_document(&mut self) {
        self.stage = Stage::Finished;
        self.locations.clear();
        self.lines.clear();
        self.atlas_children = 0;
        self.record_error = None;
        if self.text.len() > WHOLE_DOCUMENT_LIMIT {
            self.unsplit = true;
            return;
        }
        let text = std::mem::take(&mut self.text);
        match xml::parse(&text) {
            Ok(doc) => {
                let root = doc.root_element();
                self.root_name = root.tag_name().name().to_owned();
                self.is_atlas = self.root_name.eq_ignore_ascii_case("atlas");
                if self.atlas_check {
                    self.read_records = self.is_atlas;
                }
                self.read(&doc, false);
            }
            Err(e) => self.xml_error = Some(e),
        }
        self.text = text;
    }

    /// Reads one parsed piece: a child of the root, or the whole document.
    fn read(&mut self, doc: &Document<'_>, is_child: bool) {
        let top = doc.root_element();
        let exact_atlas = self.root_name == "atlas";
        if exact_atlas {
            if is_child {
                self.atlas_children += usize::from(xml::is_named(top, "location"));
            } else if xml::is_named(top, "atlas") {
                self.atlas_children += top
                    .children()
                    .filter(|c| xml::is_named(*c, "location"))
                    .count();
            }
        }
        if !self.read_records || self.record_error.is_some() {
            return;
        }
        for node in doc.descendants() {
            let goarrow_record = xml::is_named(node, "Location") || xml::is_named(node, "loc");
            if goarrow_record && self.locations.is_empty() {
                self.lines.extend(xml::own_texts(node).map(str::to_owned));
            }
            let location = if self.is_atlas {
                if !xml::is_named(node, "location") {
                    continue;
                }
                Location::from_atlas_element(node)
            } else {
                if !goarrow_record {
                    continue;
                }
                match Location::from_element(node) {
                    Ok(location) => location,
                    Err(e) => {
                        self.record_error = Some(e);
                        return;
                    }
                }
            };
            if location.has_coordinates() {
                self.locations.push(location);
                self.lines.clear();
            }
        }
    }

    /// The records, or the text lines read as `name; NS; EW` when no
    /// record had coordinates.
    fn records(&mut self) -> Result<ParsedLocations, LocationError> {
        if let Some(e) = self.record_error.take() {
            return Err(e);
        }
        let mut locations = std::mem::take(&mut self.locations);
        if locations.is_empty() {
            for line in &self.lines {
                let location = Location::from_csv_line(line)?;
                if location.has_coordinates() {
                    locations.push(location);
                }
            }
        }
        Ok(ParsedLocations {
            locations,
            is_atlas: self.is_atlas,
        })
    }
}

/// Reading a location file in steps ([`crate::database::parse_locations_xml`]).
#[derive(Debug)]
pub struct LocationsJob {
    scanner: Scanner,
    done: bool,
}

impl LocationsJob {
    /// A job over a file's text.
    #[must_use]
    pub fn new(xml: String) -> Self {
        Self {
            scanner: Scanner::new(xml, false),
            done: false,
        }
    }

    /// Does at most `max_ops` units of work (one unit is one child of the
    /// root); `Some` once done. After that it answers with an empty
    /// result.
    pub fn step(&mut self, max_ops: usize) -> Option<Result<ParsedLocations, LoadError>> {
        if self.done {
            return Some(Ok(ParsedLocations::default()));
        }
        let mut budget = max_ops;
        if !self.scanner.step(&mut budget) {
            return None;
        }
        self.done = true;
        if self.scanner.unsplit {
            return Some(Err(LoadError::Unsplit));
        }
        if let Some(e) = self.scanner.xml_error.take() {
            return Some(Err(LoadError::Xml(e)));
        }
        Some(self.scanner.records().map_err(LoadError::Location))
    }

    /// Runs to the end.
    pub fn run(mut self) -> Result<ParsedLocations, LoadError> {
        loop {
            if let Some(result) = self.step(usize::MAX) {
                return result;
            }
        }
    }
}

/// Checking and reading Atlas data in steps: what
/// [`crate::database::validate_atlas_xml`] checks, then its records
/// ([`crate::database::parse_locations_xml`]), with the location count.
#[derive(Debug)]
pub struct AtlasJob {
    scanner: Scanner,
    too_long: bool,
    done: bool,
}

impl AtlasJob {
    /// A job over downloaded or cached Atlas text.
    #[must_use]
    pub fn new(xml: String) -> Self {
        let too_long = xml.len() > ATLAS_MAX_BYTES && xml.chars().count() > ATLAS_MAX_BYTES;
        Self {
            scanner: Scanner::new(xml, true),
            too_long,
            done: false,
        }
    }

    /// Does at most `max_ops` units of work; `Some` once done.
    pub fn step(&mut self, max_ops: usize) -> Option<Result<(ParsedLocations, usize), AtlasError>> {
        if self.done {
            return Some(Ok((ParsedLocations::default(), 0)));
        }
        if self.too_long {
            self.done = true;
            return Some(Err(AtlasError::Xml(
                "The input document has exceeded a limit set by MaxCharactersInDocument."
                    .to_owned(),
            )));
        }
        let mut budget = max_ops;
        if !self.scanner.step(&mut budget) {
            return None;
        }
        self.done = true;
        let scanner = &mut self.scanner;
        if scanner.unsplit {
            return Some(Err(AtlasError::Unsplit));
        }
        if let Some(e) = scanner.xml_error.take() {
            return Some(Err(AtlasError::Xml(e)));
        }
        if !scanner.is_atlas {
            return Some(Err(AtlasError::NotAtlas));
        }
        if scanner.atlas_children == 0 {
            return Some(Err(AtlasError::NoLocations));
        }
        let count = scanner.atlas_children;
        Some(
            scanner
                .records()
                .map(|parsed| (parsed, count))
                .map_err(|e| AtlasError::Xml(e.to_string())),
        )
    }

    /// Runs to the end.
    pub fn run(mut self) -> Result<(ParsedLocations, usize), AtlasError> {
        loop {
            if let Some(result) = self.step(usize::MAX) {
                return result;
            }
        }
    }
}
