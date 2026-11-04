//! A small client for the plugin tests: the SDK's `FakeHost` for the host
//! services (log, chat, commands, clock, UI through the client's own UI
//! state), with storage, the character's position and the HTTP requests
//! answered from fields kept here.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use ac_plugin_abi::Wire;
use ac_plugin_abi::navigation::NavigationSnapshot;
use ac_plugin_abi::net::{HttpReply, HttpRequest, NetError, NetEvent};
use ac_plugin_abi::storage::{StorageKey, StorageWrite};
use ac_plugin_abi::world::Position;
use ac_plugin_ui::UiResources;
use goarrow::GoArrow;
use goarrow::settings::{SETTINGS_KEY, Settings};
use openac_plugin::testing::FakeHost;
use openac_plugin::{CommandCall, Event, Host, Import, Plugin, StartInfo, Transport};

/// Text 7 px a character, lines 16 px; the world map is 257 x 267; a PNG
/// "decodes" to its header's size, blank (the tests look at where images
/// go, not their pixels).
#[derive(Debug)]
pub struct Resources;

impl UiResources for Resources {
    fn measure_text(&self, text: &str) -> (f32, f32) {
        (text.chars().count() as f32 * 7.0, 16.0)
    }
    fn surface_size(&self, surface_id: u32) -> Option<(u32, u32)> {
        match surface_id {
            0x0600_127D => Some((257, 267)),
            0x0600_0000..=0x06FF_FFFF => Some((32, 32)),
            _ => None,
        }
    }
    fn image_dimensions(&self, bytes: &[u8]) -> Option<(u32, u32)> {
        let at = |i: usize| -> Option<u32> {
            Some(u32::from_be_bytes(bytes.get(i..i + 4)?.try_into().ok()?))
        };
        bytes
            .starts_with(b"\x89PNG\r\n\x1a\n")
            .then(|| Some((at(16)?, at(20)?)))
            .flatten()
    }
    fn decode_image(&self, bytes: &[u8]) -> Result<ac_plugin_ui::DecodedImage, String> {
        let (width, height) = self.image_dimensions(bytes).ok_or("not a PNG")?;
        Ok(ac_plugin_ui::DecodedImage {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        })
    }
}

/// Two portals to End and a town: GoArrow's `UiStartupTests` routes.
pub const ROUTES_XML: &str = r#"<locations>
  <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
  <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
  <loc name="End" type="Town" NS="0" EW="100" />
</locations>"#;

/// A small Atlas export: a portal, a town, a lifestone and a vendor.
pub const ATLAS_XML: &str = "<atlas>
  <location><id>1</id><latitude>0</latitude><longitude>1</longitude>
    <name>Holtburg to Rithwic</name><type>Town Portal</type>
    <arrival_latitude>-50</arrival_latitude><arrival_longitude>50</arrival_longitude></location>
  <location><id>2</id><latitude>-50</latitude><longitude>51</longitude>
    <name>Rithwic</name><type>Town</type></location>
  <location><id>3</id><latitude>0.5</latitude><longitude>0.5</longitude>
    <name>Holtburg</name><type>Town</type></location>
  <location><id>4</id><latitude>0.2</latitude><longitude>0.1</longitude>
    <name>Holtburg Lifestone</name><type>Lifestone</type></location>
  <location><id>5</id><latitude>0.3</latitude><longitude>0.2</longitude>
    <name>Holtburg Healer</name><type>Vendor</type></location>
</atlas>";

/// The world and the fake host.
#[derive(Debug)]
pub struct Sim {
    pub fake: FakeHost,
    /// Plugin storage: key to text.
    pub storage: BTreeMap<String, String>,
    /// The navigation snapshot.
    pub snapshot: NavigationSnapshot,
    /// HTTP requests made, in order.
    pub http: Vec<HttpRequest>,
    /// The plugin may download places at start (otherwise the settings it
    /// reads say it may not).
    pub download_at_start: bool,
}

impl Sim {
    pub fn new() -> Self {
        let mut fake = FakeHost::new("openac.goarrow");
        fake.ui.set_resources(Some(Arc::new(Resources)));
        Self {
            fake,
            storage: BTreeMap::new(),
            snapshot: NavigationSnapshot::default(),
            http: Vec::new(),
            download_at_start: false,
        }
    }

    /// Settings saved with the map shown (it starts hidden).
    pub fn show_map(&mut self) {
        self.storage
            .insert(SETTINGS_KEY.to_owned(), r#"{ "MapVisible": true, "OverlayPositionVersion": 1, "VisibilitySettingsVersion": 1 }"#.to_owned());
    }

    /// In the world, outdoors at map coordinates, facing `heading`.
    pub fn stand(&mut self, ns: f64, ew: f64, heading: f32) {
        self.snapshot.is_available = true;
        self.snapshot.is_portal_space = false;
        self.snapshot.position = Position {
            cell_id: 0x7D64_0001,
            east_west: ew,
            north_south: ns,
            elevation: 0.0,
            heading_degrees: heading,
            is_outdoor: true,
        };
    }

    /// In the world, indoors in a dungeon cell at map coordinates.
    pub fn stand_indoors(&mut self, cell_id: u32, ns: f64, ew: f64) {
        self.snapshot.is_available = true;
        self.snapshot.is_portal_space = false;
        self.snapshot.position = Position {
            cell_id,
            east_west: ew,
            north_south: ns,
            elevation: 0.0,
            heading_degrees: 0.0,
            is_outdoor: false,
        };
    }
}

fn decode<T: Wire>(bytes: &[u8]) -> T {
    T::from_bytes(bytes).expect("a request the SDK encoded")
}

impl Transport for Sim {
    fn call(&mut self, import: Import, bytes: &[u8]) -> Vec<u8> {
        let base = self.fake.call(import, bytes);
        match import {
            Import::StorageReadText => {
                let key: StorageKey = decode(bytes);
                let mut text = self.storage.get(&key.key).cloned();
                // No download at start unless a test asks: the settings the
                // plugin reads say so, without being saved.
                // A file that does not read is left as it is.
                if key.key == SETTINGS_KEY && !self.download_at_start {
                    let json = match text.as_deref() {
                        None => Some(serde_json::json!({})),
                        Some(t) => serde_json::from_str::<serde_json::Value>(t).ok(),
                    };
                    if let Some(mut json) = json.filter(serde_json::Value::is_object) {
                        json["DownloadDataAtStart"] = serde_json::Value::Bool(false);
                        text = Some(json.to_string());
                    }
                }
                Ok::<_, String>(text).to_bytes()
            }
            Import::StorageWrite => {
                let write: StorageWrite = decode(bytes);
                let text = String::from_utf8(write.bytes).expect("UTF-8");
                self.storage.insert(write.key, text);
                Ok::<(), String>(()).to_bytes()
            }
            Import::NavigationSnapshot => self.snapshot.to_bytes(),
            // The SDK's fake host does not route a decoded image to the
            // client's UI state; the client does.
            Import::UiImageDecode => {
                let owner = ac_plugin_ui::UiOwner::new(
                    self.fake.plugin_id.as_str(),
                    self.fake.display_name.as_str(),
                );
                ac_plugin_abi::ui_host::handle(
                    import,
                    bytes,
                    &owner,
                    &mut self.fake.ui,
                    None,
                    openac_plugin::API_CURRENT,
                )
                .expect("an image request")
            }
            Import::NetHttpRequest => {
                let request: HttpRequest = decode(bytes);
                self.http.push(request);
                Ok::<u32, String>(u32::try_from(self.http.len()).unwrap()).to_bytes()
            }
            _ => base,
        }
    }
}

/// The plugin on the simulated client.
#[derive(Debug)]
pub struct Harness {
    pub sim: Sim,
    pub plugin: GoArrow,
}

impl Harness {
    /// Enabled, after `setup` on the world, and ticked until its data is
    /// read.
    pub fn new(setup: impl FnOnce(&mut Sim)) -> Self {
        let mut sim = Sim::new();
        setup(&mut sim);
        let mut harness = Self {
            sim,
            plugin: GoArrow::default(),
        };
        harness.enable();
        harness.settle();
        harness
    }

    /// Enabled with the routes of [`ROUTES_XML`] loaded and the player at
    /// 0N 0E.
    pub fn with_routes() -> Self {
        let mut h = Self::new(|sim| {
            sim.storage
                .insert("GoArrow/routes.xml".to_owned(), ROUTES_XML.to_owned());
            sim.stand(0.0, 0.0, 0.0);
        });
        h.go("file routes.xml");
        h.settle();
        h.sim.fake.posted.clear();
        h
    }

    pub fn enable(&mut self) {
        let info = StartInfo {
            plugin_id: "openac.goarrow".to_owned(),
            api_version: openac_plugin::API_CURRENT.to_string(),
            client_version: "0.1.10".to_owned(),
        };
        self.plugin
            .enable(&mut Host::with_transport(&mut self.sim), &info)
            .expect("enabled");
    }

    pub fn disable(&mut self) {
        self.plugin
            .disable(&mut Host::with_transport(&mut self.sim));
    }

    pub fn tick(&mut self) {
        self.plugin.tick(&mut Host::with_transport(&mut self.sim));
    }

    /// Ticks enough for loads, searches and refreshes to finish.
    pub fn settle(&mut self) {
        for _ in 0..80 {
            self.tick();
        }
    }

    pub fn send(&mut self, event: Event) {
        self.plugin
            .on_event(&mut Host::with_transport(&mut self.sim), event);
    }

    pub fn with_host<R>(&mut self, f: impl FnOnce(&mut GoArrow, &mut Host<'_>) -> R) -> R {
        f(&mut self.plugin, &mut Host::with_transport(&mut self.sim))
    }

    /// Types `/go <arguments>`.
    pub fn go(&mut self, arguments: &str) {
        self.send(Event::Command(CommandCall {
            verb: "go".to_owned(),
            arguments: arguments.to_owned(),
            raw: format!("/go {arguments}"),
        }));
    }

    /// The chat lines posted, as the chat shows them.
    pub fn chat(&self) -> Vec<String> {
        self.sim
            .fake
            .posted
            .iter()
            .map(|p| p.text.replace("\\<", "<"))
            .collect()
    }

    /// The chat lines since the last call, and forgets them.
    pub fn take_chat(&mut self) -> Vec<String> {
        let lines = self.chat();
        self.sim.fake.posted.clear();
        lines
    }

    /// The warnings logged.
    pub fn warnings(&self) -> Vec<String> {
        self.sim
            .fake
            .logs
            .iter()
            .filter(|(level, _)| *level == openac_plugin::LogLevel::Warn)
            .map(|(_, line)| line.clone())
            .collect()
    }

    /// The settings as saved.
    pub fn saved(&self) -> Settings {
        Settings::from_json(self.sim.storage.get(SETTINGS_KEY).expect("settings saved"))
            .expect("settings")
    }

    /// The HTTP request answered with `status` and `body`.
    pub fn answer_http(&mut self, status: u32, body: &[u8]) {
        let handle = u32::try_from(self.sim.http.len()).unwrap();
        self.send(Event::Net(NetEvent::HttpCompleted(HttpReply {
            handle,
            status,
            reason: if status == 200 { "OK" } else { "Not Found" }.to_owned(),
            body: body.to_vec(),
            error: NetError::None,
            ..HttpReply::default()
        })));
    }

    // ---- The window. ----

    pub fn panel(&self) -> u32 {
        self.plugin.window().expect("the window").panel_id()
    }

    /// A control's handler fires through the client's UI state, which drops
    /// a handler the window does not bind (so the test fails here).
    pub fn act(&mut self, handler: &str, value: ac_plugin_ui::ActionValue) {
        let panel = self.panel();
        self.sim.fake.ui.input(ac_plugin_ui::UiInput::Action {
            panel,
            handler: handler.to_owned(),
            value,
        });
        let events = self.sim.fake.ui.take_events();
        assert!(
            !events.is_empty(),
            "the window binds no handler {handler} taking that argument"
        );
        for (_, event) in events {
            self.send(ac_plugin_abi::ui_host::event(event));
        }
    }

    pub fn click(&mut self, handler: &str) {
        self.act(handler, ac_plugin_ui::ActionValue::None);
    }

    pub fn type_text(&mut self, handler: &str, text: &str) {
        self.act(handler, ac_plugin_ui::ActionValue::Text(text.to_owned()));
    }

    pub fn pick(&mut self, handler: &str, index: i32) {
        self.act(handler, ac_plugin_ui::ActionValue::Int(index));
    }

    pub fn check(&mut self, handler: &str, on: bool) {
        self.act(handler, ac_plugin_ui::ActionValue::Bool(on));
    }

    pub fn value(&self, name: &str) -> openac_plugin::ui::TypedValue {
        self.sim
            .fake
            .panel_typed_value(self.panel(), name)
            .unwrap_or_else(|| panic!("no value pushed for {name}"))
    }

    pub fn text(&self, name: &str) -> String {
        match self.value(name) {
            openac_plugin::ui::TypedValue::Text(text) => text,
            other => panic!("{name} is {other:?}"),
        }
    }

    pub fn texts(&self, name: &str) -> Vec<String> {
        match self.value(name) {
            openac_plugin::ui::TypedValue::TextList(list) => list,
            other => panic!("{name} is {other:?}"),
        }
    }

    pub fn uints(&self, name: &str) -> Vec<u32> {
        match self.value(name) {
            openac_plugin::ui::TypedValue::UintList(list) => list,
            other => panic!("{name} is {other:?}"),
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        match self.value(name) {
            openac_plugin::ui::TypedValue::Bool(on) => on,
            other => panic!("{name} is {other:?}"),
        }
    }

    pub fn int(&self, name: &str) -> i32 {
        match self.value(name) {
            openac_plugin::ui::TypedValue::Int(v) => v,
            other => panic!("{name} is {other:?}"),
        }
    }

    // ---- The canvases. ----

    /// The canvas's id by its plugin name.
    pub fn canvas(&self, name: &str) -> u32 {
        self.sim
            .fake
            .ui
            .canvases()
            .find(|c| c.descriptor.canvas_id == name)
            .unwrap_or_else(|| panic!("no canvas {name}"))
            .id
    }

    /// What a canvas last painted.
    pub fn paint(&self, name: &str) -> Vec<ac_plugin_ui::PaintCmd> {
        self.sim
            .fake
            .ui
            .canvases()
            .find(|c| c.descriptor.canvas_id == name)
            .unwrap_or_else(|| panic!("no canvas {name}"))
            .paint
            .as_ref()
            .clone()
    }

    /// The texts a canvas last painted.
    pub fn painted_texts(&self, name: &str) -> Vec<String> {
        self.paint(name)
            .into_iter()
            .filter_map(|c| match c {
                ac_plugin_ui::PaintCmd::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// A pointer event as the client sends it, straight to the plugin (any
    /// kind, button and modifiers, including API 1.6's hover and leave).
    pub fn raw_pointer(&mut self, name: &str, kind: u8, at: (f32, f32), button: u8, modifiers: u8) {
        let canvas = self.canvas(name);
        self.send(Event::Ui(openac_plugin::ui::UiEvent::Pointer(
            openac_plugin::ui::PointerEvent {
                canvas,
                kind,
                x: at.0,
                y: at.1,
                button,
                modifiers,
                wheel_delta: 0,
            },
        )));
    }

    /// The pointer over a canvas with no button held, through the client's
    /// UI state (API 1.6's hover).
    pub fn hover(&mut self, name: &str, at: (f32, f32)) {
        self.ui_pointer(name, ac_plugin_ui::PointerKind::Hover, at);
    }

    /// The pointer leaving a canvas it hovered, through the client's UI
    /// state.
    pub fn leave(&mut self, name: &str) {
        self.ui_pointer(name, ac_plugin_ui::PointerKind::Leave, (0.0, 0.0));
    }

    fn ui_pointer(&mut self, name: &str, kind: ac_plugin_ui::PointerKind, at: (f32, f32)) {
        let canvas = self.canvas(name);
        let mut plugin = std::mem::take(&mut self.plugin);
        self.sim.fake.ui.input(ac_plugin_ui::UiInput::Pointer {
            canvas,
            event: ac_plugin_ui::PointerEvent {
                kind,
                x: at.0,
                y: at.1,
                button: 0,
                modifiers: 0,
                wheel_delta: 0,
            },
            place: None,
        });
        for (_, event) in self.sim.fake.ui.take_events() {
            plugin.on_event(
                &mut Host::with_transport(&mut self.sim),
                ac_plugin_abi::ui_host::event(event),
            );
        }
        self.plugin = plugin;
    }

    /// A wheel turn as the client sends it.
    pub fn raw_wheel(&mut self, name: &str, at: (f32, f32), notches: i32, modifiers: u8) {
        let canvas = self.canvas(name);
        self.send(Event::Ui(openac_plugin::ui::UiEvent::Pointer(
            openac_plugin::ui::PointerEvent {
                canvas,
                kind: openac_plugin::ui::PointerEvent::WHEEL,
                x: at.0,
                y: at.1,
                button: 0,
                modifiers,
                wheel_delta: notches,
            },
        )));
    }

    /// A click (down and up) as the client sends it.
    pub fn raw_click(&mut self, name: &str, at: (f32, f32), button: u8, modifiers: u8) {
        self.raw_pointer(name, 0, at, button, modifiers);
        self.raw_pointer(name, 1, at, button, modifiers);
    }

    /// Pointer input on a canvas, at canvas pixels.
    pub fn pointer(
        &mut self,
        name: &str,
        kind: ac_plugin_ui::PointerKind,
        x: f32,
        y: f32,
        wheel: i32,
    ) {
        let canvas = self.canvas(name);
        let mut plugin = std::mem::take(&mut self.plugin);
        self.sim.fake.ui.input(ac_plugin_ui::UiInput::Pointer {
            canvas,
            event: ac_plugin_ui::PointerEvent {
                kind,
                x,
                y,
                button: 1,
                modifiers: 0,
                wheel_delta: wheel,
            },
            place: None,
        });
        for (_, event) in self.sim.fake.ui.take_events() {
            plugin.on_event(
                &mut Host::with_transport(&mut self.sim),
                ac_plugin_abi::ui_host::event(event),
            );
        }
        self.plugin = plugin;
    }
}
