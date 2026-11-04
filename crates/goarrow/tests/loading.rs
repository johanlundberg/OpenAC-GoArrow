//! Reading location files in steps: the jobs give what the one-call
//! readers give (`LocationDatabase.LoadLocationsXml` and
//! `WarcryAtlasDataProvider.ValidateAtlasXml`), for every shape of file,
//! malformed ones included.

use goarrow::database::{
    AtlasError, LoadError, ParsedLocations, parse_locations_xml, validate_atlas_xml,
};
use goarrow::loading::{AtlasJob, LocationsJob, WHOLE_DOCUMENT_LIMIT};

fn describe(parsed: &ParsedLocations) -> (bool, Vec<String>) {
    (
        parsed.is_atlas,
        parsed.locations.iter().map(|l| l.to_xml()).collect(),
    )
}

fn stepped<T>(mut step: impl FnMut(usize) -> Option<T>, max_ops: usize) -> (T, usize) {
    let mut steps = 0;
    loop {
        steps += 1;
        if let Some(result) = step(max_ops) {
            return (result, steps);
        }
    }
}

const FILES: &[&str] = &[
    // An Atlas export: header elements, comments, CDATA, a nested record.
    "<?xml version=\"1.0\" ?>\n<atlas>\n  <copyright><notice>(c) x</notice></copyright>\n  <!-- a <location> in a comment -->\n\
     <headers><header>id</header></headers>\n\
     <location><id>1</id><name>A &amp; B</name><latitude>1</latitude><longitude>2</longitude><type>Town</type></location>\n\
     <![CDATA[ <location> ]]>\n\
     <location><id>2</id><name><![CDATA[Odd > Name]]></name><latitude>-3</latitude><longitude>4</longitude>\
     <arrival_latitude>5</arrival_latitude><arrival_longitude>6</arrival_longitude><type>Town Portal</type>\
     <description>Note 'q' \"d\"</description></location>\n\
     <group><location><id>3</id><name>Deep</name><latitude>7</latitude><longitude>8</longitude></location></group>\n\
     <location attr='a>b'/>\n</atlas>\n<!-- after -->",
    // GoArrow's formats, nested and mixed.
    "<locations><loc id='1' name='One' NS='1' EW='1'>notes</loc><group><Location name='Two'><Coords NS='2' EW='2'/></Location></group>\
     <loc name='NoCoords'/></locations>",
    // Element text read as lines.
    "<Locations><Location>Holtburg; 42.1; 33.6</Location><x><loc>Arwic;33.6;56.9</loc></x></Locations>",
    // A record as the root.
    "<loc name='Solo' NS='1' EW='2'/>",
    "<Location name='Solo'><Coords NS='1' EW='2'/></Location>",
    // An empty root, a self-closing root, a byte-order mark.
    "<Locations></Locations>",
    "<Locations/>",
    "\u{feff}<locations><loc name='B' NS='1' EW='1'/></locations>",
    // A namespace on the root.
    "<locations xmlns='urn:x'><loc name='N' NS='1' EW='1'/></locations>",
    "<p:locations xmlns:p='urn:x'><loc name='N' NS='1' EW='1'/></p:locations>",
];

const MALFORMED: &[&str] = &[
    "<locations><loc name='A' NS='1' EW='1'></locations>",
    "<locations><loc name='A' NS='1' EW='1'/>",
    "<locations><loc name='A' NS='1' EW='1'/></locations><again/>",
    "<locations><a></b></locations>",
    "<!DOCTYPE locations><locations/>",
    "text<locations/>",
    "<locations><loc name='A' NS='1' EW='1' NS='2'/></locations>",
    "<locations>&bogus;</locations>",
    "<locations><!-- unclosed </locations>",
    "",
];

// LocationDatabase.LoadLocationsXml: the job reads every file as one call.
#[test]
fn locations_job_reads_like_one_call() {
    for xml in FILES {
        let whole = parse_locations_xml(xml).unwrap();
        for max_ops in [1, 2, 1000] {
            let mut job = LocationsJob::new((*xml).to_owned());
            let (result, _) = stepped(|n| job.step(n), max_ops);
            assert_eq!(
                describe(&result.unwrap()),
                describe(&whole),
                "{xml} at {max_ops}"
            );
        }
    }
}

// XmlDocument.LoadXml throws on a malformed file; so does the job.
#[test]
fn locations_job_refuses_what_one_call_refuses() {
    for xml in MALFORMED {
        assert!(
            matches!(parse_locations_xml(xml), Err(LoadError::Xml(_))),
            "{xml}"
        );
        let mut job = LocationsJob::new((*xml).to_owned());
        let (result, _) = stepped(|n| job.step(n), 1);
        assert!(matches!(result, Err(LoadError::Xml(_))), "{xml}");
    }
}

// Location.FromXml throws on a bad indoor position, but a malformed file is
// reported first, as one parse of the whole file finds it first.
#[test]
fn a_bad_record_is_reported_after_the_file_is_known_to_be_well_formed() {
    let bad_record = "<locations><loc name='X' cellId='12340001' x='1' y='1' z='1'/><loc name='Y' NS='1' EW='1'/></locations>";
    let mut job = LocationsJob::new(bad_record.to_owned());
    let (result, _) = stepped(|n| job.step(n), 1);
    assert!(matches!(result, Err(LoadError::Location(_))));
    let both = "<locations><loc name='X' cellId='12340001' x='1' y='1' z='1'/><loc></locations>";
    let mut job = LocationsJob::new(both.to_owned());
    let (result, _) = stepped(|n| job.step(n), 1);
    assert!(matches!(result, Err(LoadError::Xml(_))));
}

// The job does one root child per unit of work.
#[test]
fn locations_job_spreads_the_children_over_steps() {
    let mut xml = String::from("<atlas>");
    for i in 0..50 {
        xml.push_str(&format!(
            "<location><id>{i}</id><name>P{i}</name><latitude>1</latitude><longitude>{i}</longitude></location>"
        ));
    }
    xml.push_str("</atlas>");
    let mut job = LocationsJob::new(xml.clone());
    let (result, steps) = stepped(|n| job.step(n), 10);
    assert_eq!(result.unwrap().locations.len(), 50);
    assert!(steps >= 5, "{steps}");
    // The Atlas and GoArrow samples split too (they are not read in one
    // piece): their children take a step each.
    for (xml, children) in [(FILES[0], 6), (FILES[1], 3)] {
        let mut job = LocationsJob::new(xml.to_owned());
        let (_, steps) = stepped(|n| job.step(n), 1);
        assert!(steps > children, "{steps} steps for {xml}");
    }
}

// WarcryAtlasDataProvider.ValidateAtlasXml, then LoadLocationsXml: the job
// gives the same verdict and, for Atlas data, the same records.
#[test]
fn atlas_job_checks_like_validate() {
    let mut cases: Vec<&str> = FILES.to_vec();
    cases.extend_from_slice(MALFORMED);
    cases.extend_from_slice(&[
        "<atlas />",
        "<atlas><location /><location /></atlas>",
        "<ATLAS><location/></ATLAS>",
        "<atlas><x><location/></x></atlas>",
        "<!DOCTYPE atlas [<!ENTITY a 'b'>]><atlas><location/></atlas>",
    ]);
    for xml in cases {
        let expected = validate_atlas_xml(xml);
        let mut job = AtlasJob::new(xml.to_owned());
        let (result, _) = stepped(|n| job.step(n), 1);
        match (&expected, &result) {
            (Ok(count), Ok((parsed, job_count))) => {
                assert_eq!(count, job_count, "{xml}");
                assert_eq!(
                    describe(parsed),
                    describe(&parse_locations_xml(xml).unwrap()),
                    "{xml}"
                );
            }
            (Err(AtlasError::Xml(_)), Err(AtlasError::Xml(_))) => {}
            (Err(a), Err(b)) => assert_eq!(a, b, "{xml}"),
            _ => panic!("{xml}: {expected:?} vs {result:?}"),
        }
    }
}

/// A file of `root` (start tag) with GoArrow records, just over 1 MB.
fn over_a_megabyte(root: &str, end: &str, record: &str) -> String {
    let mut xml = String::from(root);
    let mut i = 0;
    while xml.len() <= WHOLE_DOCUMENT_LIMIT {
        xml.push_str(&record.replace("{i}", &i.to_string()));
        i += 1;
    }
    xml.push_str(end);
    xml
}

// A file that does not split into records (a namespace on the root) is
// parsed in one piece up to 1 MB, and refused above that instead of
// holding one call for the whole parse.
#[test]
fn an_unsplittable_file_over_a_megabyte_is_refused() {
    let small = "<locations xmlns='urn:x'><loc name='P' NS='1' EW='1'/></locations>";
    assert!(LocationsJob::new(small.to_owned()).run().is_ok());
    let big = over_a_megabyte(
        "<locations xmlns='urn:x'>",
        "</locations>",
        "<loc name='Place {i}' NS='1' EW='1'/>",
    );
    assert!(matches!(
        LocationsJob::new(big).run(),
        Err(LoadError::Unsplit)
    ));
    let atlas = over_a_megabyte(
        "<atlas xmlns='urn:x'>",
        "</atlas>",
        "<location><id>{i}</id><name>P{i}</name><latitude>1</latitude><longitude>1</longitude></location>",
    );
    assert_eq!(AtlasJob::new(atlas).run().unwrap_err(), AtlasError::Unsplit);
    // One that splits is read in parts at any size.
    let split = over_a_megabyte(
        "<locations>",
        "</locations>",
        "<loc name='Place {i}' NS='1' EW='1'/>",
    );
    assert!(LocationsJob::new(split).run().unwrap().locations.len() > 20_000);
}
