//! The original GoArrow's sixteen map icons (VVS Edition `Map.resx`, MIT;
//! NOTICE.md), shipped as the PNG files they were (`assets/map/`) and
//! decoded by the client.
//!
//! They are handed over once, at the first tick a window is attached (a
//! headless session never needs them), and kept, as the shapes are
//! ([`crate::sprites`]): one the client refused is tried again when a
//! window attaches. Each is drawn at its own size times the map's icon
//! scale ([`crate::map_view::icon_scale`]).

use openac_plugin::ui::{Color, Image, Painter, rgba};
use openac_plugin::{Event, Host};

/// One of the map's icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapIcon {
    /// A town.
    Town,
    /// An outpost.
    TownSmall,
    /// A village.
    Settlement,
    /// A lifestone.
    Lifestone,
    /// A bindstone.
    Bindstone,
    /// A portal.
    Portal,
    /// A portal hub.
    PortalHub,
    /// A dungeon or an allegiance hall.
    Dungeon,
    /// A landmark.
    PointOfInterest,
    /// A person.
    Npc,
    /// A vendor.
    Store,
    /// A favourite, in place of its kind's icon.
    Favorite,
    /// Where the route starts (shipped; the original's map did not draw it,
    /// nor does this one).
    StartPoint,
    /// Where the route ends (shipped, not drawn, as the start).
    EndPoint,
    /// The arrow's destination, small (shipped; the original's map drew the
    /// larger one).
    ArrowDest,
    /// The arrow's destination.
    ArrowDest2,
}

impl MapIcon {
    /// Every icon.
    pub const ALL: [Self; 16] = [
        Self::Town,
        Self::TownSmall,
        Self::Settlement,
        Self::Lifestone,
        Self::Bindstone,
        Self::Portal,
        Self::PortalHub,
        Self::Dungeon,
        Self::PointOfInterest,
        Self::Npc,
        Self::Store,
        Self::Favorite,
        Self::StartPoint,
        Self::EndPoint,
        Self::ArrowDest,
        Self::ArrowDest2,
    ];

    /// The plugin's name for its image.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Town => "goarrow.map.town",
            Self::TownSmall => "goarrow.map.town-small",
            Self::Settlement => "goarrow.map.settlement",
            Self::Lifestone => "goarrow.map.lifestone",
            Self::Bindstone => "goarrow.map.bindstone",
            Self::Portal => "goarrow.map.portal",
            Self::PortalHub => "goarrow.map.portal-hub",
            Self::Dungeon => "goarrow.map.dungeon",
            Self::PointOfInterest => "goarrow.map.point-of-interest",
            Self::Npc => "goarrow.map.npc",
            Self::Store => "goarrow.map.store",
            Self::Favorite => "goarrow.map.favorite",
            Self::StartPoint => "goarrow.map.start-point",
            Self::EndPoint => "goarrow.map.end-point",
            Self::ArrowDest => "goarrow.map.arrow-dest",
            Self::ArrowDest2 => "goarrow.map.arrow-dest2",
        }
    }

    /// The PNG file.
    #[must_use]
    pub fn png(self) -> &'static [u8] {
        match self {
            Self::Town => include_bytes!("../assets/map/Town.png"),
            Self::TownSmall => include_bytes!("../assets/map/TownSmall.png"),
            Self::Settlement => include_bytes!("../assets/map/Settlement.png"),
            Self::Lifestone => include_bytes!("../assets/map/Lifestone.png"),
            Self::Bindstone => include_bytes!("../assets/map/Bindstone.png"),
            Self::Portal => include_bytes!("../assets/map/Portal.png"),
            Self::PortalHub => include_bytes!("../assets/map/PortalHub.png"),
            Self::Dungeon => include_bytes!("../assets/map/Dungeon.png"),
            Self::PointOfInterest => include_bytes!("../assets/map/PointOfInterest.png"),
            Self::Npc => include_bytes!("../assets/map/NPC.png"),
            Self::Store => include_bytes!("../assets/map/Store.png"),
            Self::Favorite => include_bytes!("../assets/map/Favorite.png"),
            Self::StartPoint => include_bytes!("../assets/map/StartPoint.png"),
            Self::EndPoint => include_bytes!("../assets/map/EndPoint.png"),
            Self::ArrowDest => include_bytes!("../assets/map/ArrowDest.png"),
            Self::ArrowDest2 => include_bytes!("../assets/map/ArrowDest2.png"),
        }
    }

    /// Its own size, pixels: the width and height in the PNG's header
    /// (bytes 16 to 23, big-endian), so the layout needs no decoding.
    #[must_use]
    pub fn size(self) -> (f32, f32) {
        let png = self.png();
        let at = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
        (at(16) as f32, at(20) as f32)
    }
}

/// The map's icons as the client holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MapIcons {
    images: [Image; MapIcon::ALL.len()],
    tried: bool,
}

impl MapIcons {
    /// An icon's image (none until handed over, or when refused).
    #[must_use]
    pub fn image(&self, icon: MapIcon) -> Image {
        self.images[icon as usize]
    }

    /// Whether the client holds them all.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.images.iter().all(|i| i.is_valid())
    }

    /// Hands over the icons the client does not hold yet, once a window
    /// is attached; tried once, and again when a window attaches.
    pub fn ensure(&mut self, host: &mut Host<'_>) {
        if self.tried {
            return;
        }
        self.tried = true;
        if !host.window_attached() {
            return;
        }
        for icon in MapIcon::ALL {
            let slot = &mut self.images[icon as usize];
            if !slot.is_valid() {
                *slot = host.image_decode(icon.name(), icon.png());
            }
        }
    }

    /// A window attached: hand over what is missing at the next tick.
    pub fn on_event(&mut self, event: &Event) {
        if matches!(event, Event::WindowAttached(true)) && !self.ready() {
            self.tried = false;
        }
    }

    /// Draws an icon over a rectangle (x, y, width, height), untinted;
    /// false when the client does not hold it.
    pub fn draw(&self, p: &mut Painter, icon: MapIcon, rect: (f32, f32, f32, f32)) -> bool {
        let image = self.image(icon);
        if image.is_valid() {
            p.image(image, rect.0, rect.1, rect.2, rect.3, WHITE);
        }
        image.is_valid()
    }
}

const WHITE: Color = rgba(255, 255, 255, 255);
