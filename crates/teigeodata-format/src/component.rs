//! Components (D-133, D-135): what a client chooses to download, install and load. Each section
//! kind belongs to at most one component, and a component is the set of its sections plus the
//! components it needs. This table is the one source for the forge, `check-pack`, the download
//! planner, the engine and the bindings.
//!
//! | Component | Sections | Needs | Answers |
//! |---|---|---|---|
//! | `point` | IDMAP, PLACES, `PLACE_INDEX`, REDIRECTS | | ID, lat/lon, kind; nearest, box |
//! | `elevation` | ELEVATION | point | a place's height |
//! | `admin` | HIERARCHY, `ISO_CODES` | point | a place's parents and children, ISO 3166 codes |
//! | `names` | NAMES | point | names |
//! | `search` | SEARCH | names | name search |
//! | `timezone` | `PLACE_TZ` | point, tz-rules | a place's timezone |
//! | `timezone-index` | REGION | point, tz-rules | the timezone at any coordinate |
//! | `boundaries` | ARCS, `ARCS_OVERVIEW`, POLYGONS, `POLY_INDEX` | point | outlines |
//! | `admin-index` | `ADMIN_REGION` | boundaries | the admin units at any coordinate |
//! | `postal` | POSTAL | point | postal codes |
//! | `dem` | DEM | | the height at any coordinate |
//! | `extra` | COMPONENTS, BLOBS | point | optional fields, cold data |
//! | `tz-rules` | `TZ_RULES` | | a zone's offset at an instant, a civil time's offsets |
//! | `substring` | SUBSTRING | search | name search from the middle of a word |
//! | `dem-coarse` | `DEM_COARSE` | | the height at any coordinate, from a coarse grid (D-225) |
//!
//! `BLOCK_HASHES` belongs to none: a client fetches it with any section of 1 MiB or more, whose
//! blocks it verifies (D-078).
//!
//! Within a component, [`Parts`] chooses the parts of a split kind (ADR-0040): the languages of
//! NAMES, SEARCH and SUBSTRING, and the one outline part read. The base part of the names
//! kinds is always taken.

use crate::{Kind, Part};

/// A component of a pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Component {
    /// Stable IDs, coordinates, kinds, the spatial index and retired IDs.
    Point = 0,
    /// Each place's height.
    Elevation = 1,
    /// Each place's parent and children, and the ISO 3166 codes of countries and units.
    Admin = 2,
    /// The name records.
    Names = 3,
    /// Name search.
    Search = 4,
    /// Each place's timezone area.
    Timezone = 5,
    /// The timezone area at any coordinate.
    TimezoneIndex = 6,
    /// Shared-arc outlines and their index.
    Boundaries = 7,
    /// The admin units at any coordinate.
    AdminIndex = 8,
    /// Postal records.
    Postal = 9,
    /// Elevation grids.
    Dem = 10,
    /// Optional per-place fields and cold data.
    Extra = 11,
    /// The timezones' rules (D-196): offsets at instants, civil times' offsets.
    TzRules = 12,
    /// Substring search (D-199): a name found from 3 or more characters inside a word.
    Substring = 13,
    /// A coarse elevation grid (D-225): terrain anywhere in a few hundred kilobytes.
    DemCoarse = 14,
}

impl Component {
    /// Every component, in order.
    pub const ALL: [Self; 15] = [
        Self::Point,
        Self::Elevation,
        Self::Admin,
        Self::Names,
        Self::Search,
        Self::Timezone,
        Self::TimezoneIndex,
        Self::Boundaries,
        Self::AdminIndex,
        Self::Postal,
        Self::Dem,
        Self::Extra,
        Self::TzRules,
        Self::Substring,
        Self::DemCoarse,
    ];

    /// Its name, as users, recipes and bindings spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Elevation => "elevation",
            Self::Admin => "admin",
            Self::Names => "names",
            Self::Search => "search",
            Self::Timezone => "timezone",
            Self::TimezoneIndex => "timezone-index",
            Self::Boundaries => "boundaries",
            Self::AdminIndex => "admin-index",
            Self::Postal => "postal",
            Self::Dem => "dem",
            Self::Extra => "extra",
            Self::TzRules => "tz-rules",
            Self::Substring => "substring",
            Self::DemCoarse => "dem-coarse",
        }
    }

    /// The component named `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The section kinds that belong to it.
    #[must_use]
    pub const fn kinds(self) -> &'static [Kind] {
        match self {
            Self::Point => &[
                Kind::IDMAP,
                Kind::PLACES,
                Kind::PLACE_INDEX,
                Kind::REDIRECTS,
            ],
            Self::Elevation => &[Kind::ELEVATION],
            Self::Admin => &[Kind::HIERARCHY, Kind::ISO_CODES],
            Self::Names => &[Kind::NAMES],
            Self::Search => &[Kind::SEARCH],
            Self::Timezone => &[Kind::PLACE_TZ],
            Self::TimezoneIndex => &[Kind::REGION],
            Self::Boundaries => &[
                Kind::ARCS,
                Kind::ARCS_OVERVIEW,
                Kind::POLYGONS,
                Kind::POLY_INDEX,
            ],
            Self::AdminIndex => &[Kind::ADMIN_REGION],
            Self::Postal => &[Kind::POSTAL],
            Self::Dem => &[Kind::DEM],
            Self::Extra => &[Kind::COMPONENTS, Kind::BLOBS],
            Self::TzRules => &[Kind::TZ_RULES],
            Self::Substring => &[Kind::SUBSTRING],
            Self::DemCoarse => &[Kind::DEM_COARSE],
        }
    }

    /// The components it needs directly.
    #[must_use]
    pub const fn needs(self) -> &'static [Self] {
        match self {
            Self::Point | Self::Dem | Self::DemCoarse | Self::TzRules => &[],
            Self::Search => &[Self::Names],
            // Its matches are ranked with search's, from SEARCH's importance (D-199).
            Self::Substring => &[Self::Search],
            Self::AdminIndex => &[Self::Boundaries],
            // A zone without its rules is a broken answer, and the rules are a few kB (D-196).
            Self::Timezone | Self::TimezoneIndex => &[Self::Point, Self::TzRules],
            Self::Elevation
            | Self::Admin
            | Self::Names
            | Self::Boundaries
            | Self::Postal
            | Self::Extra => &[Self::Point],
        }
    }

    /// The component section `kind` belongs to; `None` for `BLOCK_HASHES` and unknown kinds.
    #[must_use]
    pub fn of(kind: Kind) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.kinds().contains(&kind))
    }

    const fn bit(self) -> u16 {
        1 << self as u8
    }
}

/// A set of components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Components(u16);

impl Components {
    /// No component.
    pub const NONE: Self = Self(0);

    /// Every component.
    #[must_use]
    pub fn all() -> Self {
        Self::of(&Component::ALL)
    }

    /// The set of `list`, without what they need (see [`Self::closed`]).
    #[must_use]
    pub fn of(list: &[Component]) -> Self {
        Self(list.iter().fold(0, |m, c| m | c.bit()))
    }

    /// The components in either set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The components in `self` but not in `other`.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether `c` is in the set.
    #[must_use]
    pub const fn contains(self, c: Component) -> bool {
        self.0 & c.bit() != 0
    }

    /// Whether every component of `other` is in the set.
    #[must_use]
    pub const fn contains_all(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set with everything its components need, transitively.
    #[must_use]
    pub fn closed(self) -> Self {
        let mut m = self.0;
        loop {
            let next = Component::ALL
                .into_iter()
                .filter(|c| m & c.bit() != 0)
                .flat_map(|c| c.needs().iter())
                .fold(m, |m, d| m | d.bit());
            if next == m {
                return Self(m);
            }
            m = next;
        }
    }

    /// Its components, in order.
    pub fn iter(self) -> impl Iterator<Item = Component> {
        Component::ALL
            .into_iter()
            .filter(move |&c| self.contains(c))
    }

    /// Whether section `kind` belongs to one of its components. `BLOCK_HASHES` belongs to none,
    /// so this is `false` for it.
    #[must_use]
    pub fn holds(self, kind: Kind) -> bool {
        Component::of(kind).is_some_and(|c| self.contains(c))
    }
}

/// The most languages a [`Parts`] names.
pub const MAX_LANGUAGES: usize = 8;

/// Which parts of the split kinds a client takes (ADR-0040, D-251): what an install fetches and
/// what a layer reads. The default takes every part.
///
/// - **Names** (NAMES, SEARCH, SUBSTRING): the base part (canonical names and names with no
///   language) and, when languages are named, only their parts. A language the pack gives no
///   part of its own lives in [`Part::REST`]: [`Parts::resolve`] maps it there once the pack's
///   parts are known.
/// - **Outlines** (ARCS, `ARCS_OVERVIEW`, POLYGONS, `POLY_INDEX`, `ADMIN_REGION`): every part,
///   or one, whole for its levels ([`Part::outline`], or the base for every level at full
///   resolution).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Parts {
    /// The language parts taken besides the base, `None` for every one.
    languages: Option<([Part; MAX_LANGUAGES], u8)>,
    /// The one outline part taken, `None` for every one.
    outline: Option<Part>,
}

impl Parts {
    /// Every part.
    pub const ALL: Self = Self {
        languages: None,
        outline: None,
    };

    /// Only the base names part and the parts of `languages` (each [`Part::language`] or
    /// [`Part::REST`]); `None` for more than [`MAX_LANGUAGES`].
    #[must_use]
    pub fn languages(self, languages: &[Part]) -> Option<Self> {
        let mut list = [Part::BASE; MAX_LANGUAGES];
        for (slot, &p) in list.iter_mut().zip(languages) {
            *slot = p;
        }
        let n = u8::try_from(languages.len())
            .ok()
            .filter(|&n| usize::from(n) <= MAX_LANGUAGES)?;
        Some(Self {
            languages: Some((list, n)),
            ..self
        })
    }

    /// Only outline part `part`.
    #[must_use]
    pub const fn outline(self, part: Part) -> Self {
        Self {
            outline: Some(part),
            ..self
        }
    }

    /// The one outline part taken, if one is chosen.
    #[must_use]
    pub const fn outline_part(self) -> Option<Part> {
        self.outline
    }

    /// The language parts taken besides the base, if languages are chosen.
    #[must_use]
    pub fn language_parts(&self) -> Option<&[Part]> {
        self.languages
            .as_ref()
            .map(|(list, n)| list.get(..usize::from(*n)).unwrap_or_default())
    }

    /// These parts on a pack that `has` a names part or not: each language chosen without a
    /// part of its own becomes [`Part::REST`] (the base when the pack has no `*` either).
    #[must_use]
    pub fn resolve(self, has: impl Fn(Part) -> bool) -> Self {
        let Some((mut list, n)) = self.languages else {
            return self;
        };
        let has_rest = has(Part::REST);
        for p in list.iter_mut().take(usize::from(n)) {
            if !p.is_base() && *p != Part::REST && !has(*p) {
                *p = if has_rest { Part::REST } else { Part::BASE };
            }
        }
        Self {
            languages: Some((list, n)),
            ..self
        }
    }

    /// Whether section `kind`, part `part`, is taken.
    #[must_use]
    pub fn holds(&self, kind: Kind, part: Part) -> bool {
        if kind.is_outline() {
            return self.outline.is_none_or(|o| o == part);
        }
        if part.is_base() || !matches!(kind, Kind::NAMES | Kind::SEARCH | Kind::SUBSTRING) {
            return true;
        }
        self.language_parts().is_none_or(|l| l.contains(&part))
    }
}

/// What a client installs of a pack: its components and, within them, the parts of the split
/// kinds (D-251). From a [`Components`] alone it takes every part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    /// The components.
    pub set: Components,
    /// The parts within them.
    pub parts: Parts,
}

impl From<Components> for Selection {
    fn from(set: Components) -> Self {
        Self {
            set,
            parts: Parts::ALL,
        }
    }
}

impl Selection {
    /// The selection with its components closed under their needs.
    #[must_use]
    pub fn closed(self) -> Self {
        Self {
            set: self.set.closed(),
            ..self
        }
    }

    /// Whether section `kind`, part `part`, is installed: a section of no component always
    /// (`BLOCK_HASHES` is the caller's to decide), else its component's and its part's.
    #[must_use]
    pub fn holds(&self, kind: Kind, part: Part) -> bool {
        Component::of(kind).is_none_or(|c| self.set.contains(c)) && self.parts.holds(kind, part)
    }
}

#[cfg(test)]
mod tests {
    use super::{Component, Components, MAX_LANGUAGES, Parts};
    use crate::{KINDS, Kind, Part};

    #[test]
    fn parts_take_the_languages_and_the_outline_chosen() {
        let lang = |t| Part::language(t).unwrap();
        let (ne, en) = (lang("ne"), lang("en"));
        let all = Parts::default();
        assert!(all.holds(Kind::NAMES, ne) && all.holds(Kind::ARCS, Part::outline(2, 3).unwrap()));
        // English only, on a pack whose parts are base, `*` and `ne`: English lives in `*`.
        let p = Parts::ALL
            .languages(&[en])
            .unwrap()
            .resolve(|q| [Part::BASE, Part::REST, ne].contains(&q));
        for k in [Kind::NAMES, Kind::SEARCH, Kind::SUBSTRING] {
            assert!(p.holds(k, Part::BASE) && p.holds(k, Part::REST) && !p.holds(k, ne));
        }
        // A pack without `*`: the language has only the base part's names.
        let p = Parts::ALL
            .languages(&[en])
            .unwrap()
            .resolve(|q| [Part::BASE, ne].contains(&q));
        assert_eq!(p.language_parts(), Some(&[Part::BASE][..]));
        assert!(!p.holds(Kind::NAMES, ne));
        // A language with its own part keeps it.
        let p = Parts::ALL
            .languages(&[ne])
            .unwrap()
            .resolve(|q| [Part::BASE, Part::REST, ne].contains(&q));
        assert!(p.holds(Kind::NAMES, ne) && !p.holds(Kind::NAMES, Part::REST));
        // One outline part: only it, the base included in the choice.
        let d = Part::outline(2, 2).unwrap();
        let p = Parts::ALL.outline(d);
        assert!(p.holds(Kind::ADMIN_REGION, d) && !p.holds(Kind::ADMIN_REGION, Part::BASE));
        assert!(!p.holds(Kind::ARCS, Part::outline(1, 2).unwrap()));
        assert!(Parts::ALL.outline(Part::BASE).holds(Kind::ARCS, Part::BASE));
        // Other kinds take only their base part, always held.
        assert!(p.holds(Kind::PLACES, Part::BASE));
        assert!(Parts::ALL.languages(&[en; MAX_LANGUAGES + 1]).is_none());
    }

    #[test]
    fn every_kind_but_block_hashes_has_one_component() {
        for &(k, name) in KINDS {
            let owners: alloc::vec::Vec<Component> = Component::ALL
                .into_iter()
                .filter(|c| c.kinds().contains(&k))
                .collect();
            if k == Kind::BLOCK_HASHES {
                assert_eq!(owners, alloc::vec::Vec::<Component>::new());
            } else {
                assert_eq!(owners.len(), 1, "{name}: {owners:?}");
            }
            assert_eq!(Component::of(k), owners.first().copied(), "{name}");
        }
        assert_eq!(Component::of(Kind(99)), None);
    }

    #[test]
    fn names_round_trip_and_needs_close_without_cycles() {
        for (i, c) in Component::ALL.into_iter().enumerate() {
            assert_eq!(c as usize, i);
            assert_eq!(Component::from_name(c.name()), Some(c));
            // No component needs itself through the others, so the graph has no cycle.
            let through = Components::of(c.needs()).closed();
            assert!(!through.contains(c), "{c:?}");
        }
        assert_eq!(Component::from_name("points"), None);
        let s = Components::of(&[Component::Search, Component::AdminIndex]).closed();
        let want = [
            Component::Point,
            Component::Names,
            Component::Search,
            Component::Boundaries,
            Component::AdminIndex,
        ];
        assert!(s.iter().eq(want));
        assert!(s.holds(Kind::ARCS) && s.holds(Kind::IDMAP) && !s.holds(Kind::REGION));
        assert!(!Components::all().holds(Kind::BLOCK_HASHES));
        assert!(
            Components::of(&[Component::Dem])
                .closed()
                .iter()
                .eq([Component::Dem])
        );
        assert!(Components::NONE.is_empty() && Components::NONE.closed().is_empty());
        let tz = Components::of(&[Component::Timezone]).closed();
        assert!(
            tz.iter()
                .eq([Component::Point, Component::Timezone, Component::TzRules])
        );
        assert!(
            Components::of(&[Component::TzRules])
                .closed()
                .iter()
                .eq([Component::TzRules])
        );
        // Substring search ranks with search, and so needs it and what it needs (D-199).
        assert!(Components::of(&[Component::Substring]).closed().iter().eq([
            Component::Point,
            Component::Names,
            Component::Search,
            Component::Substring
        ]));
        let names = Components::of(&[Component::Names]);
        assert_eq!(s.without(names).union(names), s);
        assert!(!s.without(names).contains(Component::Names));
    }

    /// pack-format.md names each kind's component in its Sections table.
    #[test]
    fn the_spec_names_each_kinds_component() {
        let spec = include_str!("../../../docs/04-architecture/pack-format.md");
        for &(k, name) in KINDS {
            let row = spec
                .lines()
                .find(|l| l.starts_with(&alloc::format!("| {} | {name} |", k.0)))
                .unwrap_or_else(|| panic!("no row for {name}"));
            let cell = row.split('|').nth(4).map(str::trim);
            let want = Component::of(k).map_or_else(
                || alloc::string::String::from("–"),
                |c| alloc::format!("`{}`", c.name()),
            );
            assert_eq!(cell, Some(want.as_str()), "{name}");
        }
    }
}
