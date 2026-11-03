//! Every choice of a conversion, with the defaults the decision chose and
//! the combinations it refuses.

use crate::error::Error;
use neoteletext_core::consts::{COLOUR_BLUE_BIT, COLOUR_GREEN_BIT, COLOUR_RED_BIT, COLOURS, PRIMARY_FULL};

/// A colour of eight bits a primary: red, green, blue.
pub type Rgb8 = [u8; 3];

/// The presentation level the page is written for (EN 300 706 clause 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// Level 1: the eight colours of CLUT 0, set by spacing attributes.
    One,
    /// Level 1.5: encoded as Level 1, since what it adds are characters,
    /// none of them of use to a picture.
    OnePointFive,
    /// Level 2.5: CLUTs 2 and 3 redefined, colours set by X/26 triplets.
    TwoPointFive,
    /// Level 3.5: all four CLUTs but black, white and transparent
    /// redefined, colours set by X/26 triplets.
    #[default]
    ThreePointFive,
}

impl Level {
    /// Whether the level sets colours by X/26 triplets and redefines CLUTs.
    #[must_use]
    pub const fn is_enhanced(self) -> bool {
        matches!(self, Self::TwoPointFive | Self::ThreePointFive)
    }

    /// The level as its number is written: "1", "1.5", "2.5" or "3.5".
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::OnePointFive => "1.5",
            Self::TwoPointFive => "2.5",
            Self::ThreePointFive => "3.5",
        }
    }

    /// The level named by its number.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::One, Self::OnePointFive, Self::TwoPointFive, Self::ThreePointFive]
            .into_iter()
            .find(|level| level.name() == text)
    }
}

/// The display whose shape the page is cropped to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    /// The engine's own rendering, whose dots are square.
    #[default]
    Square,
    /// A 4:3 television, whose dots are 16/13 as wide as they are tall
    /// (the core's `ASPECT_NUMERATOR` and `ASPECT_DENOMINATOR`).
    Television,
}

/// The filter that scales the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Smooth {
    /// Each output dot the mean of the area it covers.
    #[default]
    Area,
    /// A Lanczos filter of three lobes: sharper, with slight ringing.
    Lanczos,
}

/// How the picture's shape is made the page's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    /// The picture fills the page and what overhangs is cut off.
    #[default]
    Crop,
    /// The whole picture is shown, between bars of the background colour.
    Contain,
    /// The picture fills the page and is distorted to fit.
    Stretch,
}

/// Where a part sits along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The left or top.
    Start,
    /// The middle.
    Middle,
    /// The right or bottom.
    End,
}

impl Place {
    /// The offset of a part with `free` positions to spare.
    #[must_use]
    pub const fn offset(self, free: usize) -> usize {
        match self {
            Self::Start => 0,
            Self::Middle => free / 2,
            Self::End => free,
        }
    }
}

/// Where the picture sits: which part survives a crop, or where it lies
/// between bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Align {
    /// Across the page.
    pub across: Place,
    /// Down the page.
    pub down: Place,
}

impl Align {
    /// The middle of the page.
    pub const CENTRE: Self = Self { across: Place::Middle, down: Place::Middle };
    /// The bottom middle: where a picture 72 to 74 sixels tall sits by
    /// default, leaving its spare rows at the top.
    pub const BOTTOM: Self = Self { across: Place::Middle, down: Place::End };
    /// Every alignment with its name, centre first.
    pub const NAMED: [(&'static str, Self); 9] = [
        ("center", Self::CENTRE),
        ("top", Self { across: Place::Middle, down: Place::Start }),
        ("bottom", Self::BOTTOM),
        ("left", Self { across: Place::Start, down: Place::Middle }),
        ("right", Self { across: Place::End, down: Place::Middle }),
        ("top-left", Self { across: Place::Start, down: Place::Start }),
        ("top-right", Self { across: Place::End, down: Place::Start }),
        ("bottom-left", Self { across: Place::Start, down: Place::End }),
        ("bottom-right", Self { across: Place::End, down: Place::End }),
    ];

    /// The alignment of a name in [`Align::NAMED`], or "centre".
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text == "centre" {
            return Some(Self::CENTRE);
        }
        Self::NAMED.iter().find(|(name, _)| *name == text).map(|(_, align)| *align)
    }
}

/// How the crop window is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// By the alignment.
    #[default]
    Align,
    /// Where the picture has the most detail: the summed differences of
    /// brightness between neighbouring pixels, nearest the centre on equal
    /// totals. A heuristic, which can prefer a busy background.
    Detail,
}

/// A region of the picture, in its own pixels after orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// The left edge.
    pub x: usize,
    /// The top edge.
    pub y: usize,
    /// The width, at least 1.
    pub width: usize,
    /// The height, at least 1.
    pub height: usize,
}

impl Region {
    /// A region written `X,Y,WIDTH,HEIGHT`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(',').map(|part| part.trim().parse::<usize>());
        let (Some(Ok(x)), Some(Ok(y)), Some(Ok(width)), Some(Ok(height)), None) =
            (parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        Some(Self { x, y, width, height })
    }
}

/// Every choice of a conversion. [`Options::default`] is the page most
/// receivers show as drawn: Level 3.5 with its rows in the page's own X/26,
/// cropped to fill the page, centred, on black.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "each flag is one independent choice of the command line")]
pub struct Options {
    /// The presentation level.
    pub level: Level,
    /// Levels 2.5 and 3.5: carry the rows' triplets in Active objects on
    /// a Public Object Page, 1A0, which holds far more of them than the
    /// page's own 208; the engine draws them only when built with objects.
    pub objects: bool,
    /// Levels 1 and 1.5: Hold Mosaics, so an attribute's cell shows the
    /// last mosaic rather than the background.
    pub hold_mosaics: bool,
    /// The display the shape is made for.
    pub display: Display,
    /// The scaling filter.
    pub smooth: Smooth,
    /// How the shape is fitted.
    pub fit: Fit,
    /// Where the picture sits; `None` is centred, or for a picture of 80 by
    /// 72 to 74 sixels taken as drawn, the bottom.
    pub align: Option<Align>,
    /// The colour of bars and of transparent areas.
    pub background: Rgb8,
    /// Take the picture sixel for sixel, enlarged by the largest whole
    /// number that fits.
    pub pixel_art: bool,
    /// Turn the picture upright by its orientation tag.
    pub auto_orient: bool,
    /// Cut this region out first.
    pub crop: Option<Region>,
    /// Allow enlarging; without it a small picture keeps its own size.
    pub upscale: bool,
    /// How the crop window is chosen.
    pub focus: Focus,
    /// Draw rows 1 to 24 only and leave row 0 to the page's header.
    pub keep_header: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            level: Level::default(),
            objects: false,
            hold_mosaics: false,
            display: Display::default(),
            smooth: Smooth::default(),
            fit: Fit::default(),
            align: None,
            background: BLACK,
            pixel_art: false,
            auto_orient: true,
            crop: None,
            upscale: true,
            focus: Focus::default(),
            keep_header: false,
        }
    }
}

/// Black, the default background.
const BLACK: Rgb8 = [0; 3];

impl Options {
    /// Refuses the combinations that contradict each other.
    pub fn check(&self) -> Result<(), Error> {
        let refused = if self.hold_mosaics && self.level.is_enhanced() {
            Some("hold mosaics is for Levels 1 and 1.5")
        } else if self.objects && !self.level.is_enhanced() {
            Some("objects are for Levels 2.5 and 3.5")
        } else if self.pixel_art && (self.fit != Fit::Crop || self.focus != Focus::Align) {
            Some("pixel art draws sixel for sixel, so a fit and a focus do not apply")
        } else if !self.upscale && self.fit == Fit::Stretch {
            Some("stretch always scales, so it cannot keep the picture's size")
        } else if self.focus == Focus::Detail && (self.fit != Fit::Crop || self.align.is_some() || !self.upscale) {
            Some("focus on detail chooses the crop itself: not with contain, stretch, an alignment or no enlarging")
        } else {
            None
        };
        refused.map_or(Ok(()), |reason| Err(Error::Conflict(reason)))
    }
}

/// The Level 1 colour names in colour-number order (EN 300 706 Table 26).
const COLOUR_NAMES: [&str; COLOURS] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
/// Hexadecimal digits of a `#RRGGBB` colour.
const HEX_DIGITS: usize = 6;
/// Hexadecimal digits of one primary.
const HEX_PRIMARY_DIGITS: usize = 2;
/// The radix of a hexadecimal digit.
const HEX_RADIX: u32 = 16;

/// A Level 1 colour name, or `#RRGGBB`.
#[must_use]
pub fn parse_colour(text: &str) -> Option<Rgb8> {
    if let Some(number) = COLOUR_NAMES.iter().position(|name| *name == text) {
        let primary = |bit: u8| if number & usize::from(bit) != 0 { PRIMARY_FULL } else { 0 };
        return Some([primary(COLOUR_RED_BIT), primary(COLOUR_GREEN_BIT), primary(COLOUR_BLUE_BIT)]);
    }
    let hex = text.strip_prefix('#')?;
    if hex.len() != HEX_DIGITS || !hex.is_ascii() {
        return None;
    }
    let mut colour = BLACK;
    for (primary, digits) in colour.iter_mut().zip(hex.as_bytes().chunks(HEX_PRIMARY_DIGITS)) {
        *primary = u8::from_str_radix(core::str::from_utf8(digits).ok()?, HEX_RADIX).ok()?;
    }
    Some(colour)
}
