//! Location data (`WarcryAtlasDataProvider.cs`, `GoArrowPlugin.UpdateDataAsync`;
//! GoArrow's `WarcryAtlasDataProviderTests`): the download through the
//! client's HTTP, its checks, the cache and its metadata, and the cache
//! standing in for a failed download.

mod common;

use common::{ATLAS_XML, Harness};
use goarrow::data::{CACHE_KEY, CacheMetadata, METADATA_KEY};
use goarrow::settings::SETTINGS_KEY;

const URL: &str = "https://raw.githubusercontent.com/example/data/master/atlas.xml";

fn with_url() -> Harness {
    let mut h = Harness::new(|sim| sim.fake.clock.utc_unix_ms = 1_793_000_000_000);
    h.go(&format!("url {URL}"));
    h.take_chat();
    h
}

#[test]
fn a_download_is_checked_cached_and_loaded() {
    let mut h = with_url();
    h.go("update");
    assert_eq!(h.take_chat(), ["GoArrow: Downloading location data..."]);
    let request = h.sim.http.last().expect("a request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, URL);
    assert_eq!(request.max_bytes, 16 * 1024 * 1024);
    assert!(request.follow_redirects);
    h.go("update");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: A location-data update is already running."]
    );
    h.answer_http(200, ATLAS_XML.as_bytes());
    h.settle();
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Loaded 5 locations from the Atlas data source."]
    );
    assert_eq!(h.plugin.database().location_count(), 5);
    assert_eq!(h.sim.storage[CACHE_KEY], ATLAS_XML);
    let metadata: CacheMetadata = serde_json::from_str(&h.sim.storage[METADATA_KEY]).unwrap();
    assert_eq!(metadata.location_count, 5);
    assert_eq!(metadata.source_url, URL);
    assert_eq!(metadata.downloaded_at, "2026-10-26T07:33:20+00:00");
    assert!(!h.plugin.is_updating());
    assert_eq!(h.plugin.data_status(0), "Loaded 5 locations.");
    h.with_host(|app, _| app.clear_location_status());
    assert_eq!(h.plugin.data_status(0), "5 places · downloaded 26 Oct 2026");
    // The next start reads the cache.
    let storage = h.sim.storage.clone();
    let again = Harness::new(|sim| sim.storage = storage);
    assert_eq!(again.plugin.database().location_count(), 5);
}

#[test]
fn a_failed_download_falls_back_to_a_fresh_cache() {
    let mut h = with_url();
    h.go("update");
    h.answer_http(200, ATLAS_XML.as_bytes());
    h.settle();
    h.take_chat();
    h.go("update");
    h.answer_http(404, b"");
    h.settle();
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Downloading location data...",
            "GoArrow: Download failed; loaded 5 cached locations."
        ]
    );
    assert_eq!(
        h.plugin.data_status(0),
        "Download failed; loaded cached data."
    );
    // Older than AtlasCacheMaxAgeDays: not used.
    h.sim.fake.clock.utc_unix_ms += 31 * 86_400_000;
    h.go("update");
    h.answer_http(200, b"<locations/>");
    h.settle();
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Downloading location data...",
            "GoArrow: Location-data update failed; keeping the existing database."
        ]
    );
    assert_eq!(h.plugin.database().location_count(), 5);
    assert_eq!(
        h.plugin.data_status(0),
        "Download failed; existing data kept."
    );
}

#[test]
fn without_a_cache_a_failure_keeps_the_existing_data() {
    for body in [&b""[..], b"<atlas/>", b"not xml"] {
        let mut h = with_url();
        h.go("update");
        h.answer_http(200, body);
        h.settle();
        assert_eq!(
            h.take_chat(),
            [
                "GoArrow: Downloading location data...",
                "GoArrow: Location-data update failed; keeping the existing database."
            ],
            "{body:?}"
        );
        assert!(!h.sim.storage.contains_key(CACHE_KEY));
        assert!(
            h.sim
                .fake
                .logs
                .iter()
                .any(|(_, l)| l.starts_with("GoArrow: Location-data update failed.")),
            "{:?}",
            h.sim.fake.logs
        );
    }
}

#[test]
fn too_large_a_download_is_refused() {
    let mut h = with_url();
    h.go("update");
    h.answer_http(200, &vec![b' '; 16 * 1024 * 1024 + 1]);
    h.settle();
    assert!(
        h.sim
            .fake
            .logs
            .iter()
            .any(|(_, l)| l.contains("larger than the 16 MiB limit")),
        "{:?}",
        h.sim.fake.logs
    );
}

#[test]
fn the_options_tab_downloads_from_the_address_typed() {
    let mut h = Harness::new(|_| {});
    h.click("ShowOptions");
    h.click("UseSuggestedData");
    assert_eq!(
        h.text("DataUrl"),
        "https://raw.githubusercontent.com/Darktorizo/GoArrow_Data_CoD/master/data_cod.xml"
    );
    // Filling in is not saving.
    assert_eq!(h.plugin.settings().external_data_url, "");
    h.type_text("SetDataUrl", "not an address");
    h.click("Download");
    assert_eq!(
        h.text("DataStatus"),
        "Enter a valid http:// or https:// URL."
    );
    assert!(h.sim.http.is_empty());
    h.click("UseSuggestedData");
    h.click("Download");
    assert_eq!(h.sim.http.len(), 1);
    assert_eq!(h.text("DataStatus"), "Downloading location data...");
    assert_eq!(
        h.saved().external_data_url,
        "https://raw.githubusercontent.com/Darktorizo/GoArrow_Data_CoD/master/data_cod.xml"
    );
}

const SUGGESTED: &str =
    "https://raw.githubusercontent.com/Darktorizo/GoArrow_Data_CoD/master/data_cod.xml";

#[test]
fn a_fresh_install_downloads_the_suggested_data_at_start() {
    let mut h = Harness::new(|sim| {
        sim.fake.clock.utc_unix_ms = 1_793_000_000_000;
        sim.download_at_start = true;
    });
    let request = h.sim.http.last().expect("a request at start");
    assert_eq!(request.url, SUGGESTED);
    assert_eq!(h.saved().external_data_url, SUGGESTED);
    assert_eq!(
        h.take_chat(),
        [
            format!(
                "GoArrow: No places yet, so downloading them from {SUGGESTED} (change the address in Options)."
            ),
            "GoArrow: Downloading location data...".to_owned(),
        ]
    );
    h.answer_http(200, ATLAS_XML.as_bytes());
    h.settle();
    assert_eq!(h.plugin.database().location_count(), 5);
    assert_eq!(h.sim.storage[CACHE_KEY], ATLAS_XML);
}

#[test]
fn a_start_without_places_downloads_from_the_saved_address() {
    let mut h = Harness::new(|sim| {
        sim.storage.insert(
            SETTINGS_KEY.to_owned(),
            format!(r#"{{ "ExternalDataUrl": "{URL}" }}"#),
        );
        sim.download_at_start = true;
    });
    assert_eq!(h.sim.http.len(), 1);
    assert_eq!(h.sim.http[0].url, URL);
    assert_eq!(
        h.take_chat(),
        [
            format!(
                "GoArrow: No places yet, so downloading them from {URL} (change the address in Options)."
            ),
            "GoArrow: Downloading location data...".to_owned(),
        ]
    );
}

#[test]
fn a_start_with_cached_places_downloads_nothing() {
    let h = Harness::new(|sim| {
        sim.storage
            .insert(CACHE_KEY.to_owned(), ATLAS_XML.to_owned());
        sim.download_at_start = true;
    });
    assert!(h.sim.http.is_empty());
    assert_eq!(h.plugin.database().location_count(), 5);
}

#[test]
fn the_setting_turns_the_download_at_start_off() {
    let h = Harness::new(|sim| {
        sim.storage.insert(
            SETTINGS_KEY.to_owned(),
            r#"{ "DownloadDataAtStart": false }"#.to_owned(),
        );
        sim.download_at_start = true;
    });
    assert!(h.sim.http.is_empty());
    assert_eq!(h.plugin.settings().external_data_url, "");
}
