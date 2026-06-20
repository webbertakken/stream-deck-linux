//! Stream Deck hardware models and their image specifications.
//!
//! Only the MK.2 is fully exercised against real hardware here, but the
//! registry is shaped so further models slot in without touching call sites.

/// Elgato USB vendor id shared by every Stream Deck.
pub const ELGATO_VENDOR_ID: u16 = 0x0fd9;

/// Encoded image format a model expects for its keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Baseline JPEG (MK.2 and other V2 devices).
    Jpeg,
}

/// How a key image must be oriented before upload.
///
/// Flips are applied first, then a clockwise quarter-turn rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orientation {
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub rotate_quarter_turns: u8,
}

/// Per-key image requirements for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub orientation: Orientation,
}

/// A concrete Stream Deck model definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub name: &'static str,
    pub product_id: u16,
    pub key_count: u8,
    pub columns: u8,
    pub rows: u8,
    pub image: ImageSpec,
}

impl Model {
    /// The Stream Deck MK.2: 15 keys (5x3), 72x72 JPEG rotated 180 degrees.
    pub const MK2: Model = Model {
        name: "Stream Deck MK.2",
        product_id: 0x0080,
        key_count: 15,
        columns: 5,
        rows: 3,
        image: ImageSpec {
            width: 72,
            height: 72,
            format: ImageFormat::Jpeg,
            orientation: Orientation {
                flip_horizontal: true,
                flip_vertical: true,
                rotate_quarter_turns: 0,
            },
        },
    };

    /// Every model the library knows about.
    pub const ALL: &'static [Model] = &[Model::MK2];

    /// Look up a model by its USB product id.
    pub fn from_product_id(product_id: u16) -> Option<Model> {
        Model::ALL
            .iter()
            .copied()
            .find(|m| m.product_id == product_id)
    }

    /// Hardware key index for a grid position, or `None` if out of range.
    ///
    /// The MK.2 numbers keys left-to-right, top-to-bottom with no remapping.
    pub fn key_index(&self, row: u8, column: u8) -> Option<u8> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        Some(row * self.columns + column)
    }

    /// Whether a hardware key index is valid for this model.
    pub fn is_valid_key(&self, index: u8) -> bool {
        index < self.key_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mk2_lookup_by_product_id() {
        assert_eq!(Model::from_product_id(0x0080), Some(Model::MK2));
        assert_eq!(Model::from_product_id(0xffff), None);
    }

    #[test]
    fn mk2_grid_dimensions_are_consistent() {
        let m = Model::MK2;
        assert_eq!(m.columns as u16 * m.rows as u16, m.key_count as u16);
    }

    #[test]
    fn key_index_maps_grid_left_to_right_top_to_bottom() {
        let m = Model::MK2;
        assert_eq!(m.key_index(0, 0), Some(0));
        assert_eq!(m.key_index(0, 4), Some(4));
        assert_eq!(m.key_index(1, 0), Some(5));
        assert_eq!(m.key_index(2, 4), Some(14));
    }

    #[test]
    fn key_index_rejects_out_of_grid() {
        let m = Model::MK2;
        assert_eq!(m.key_index(3, 0), None);
        assert_eq!(m.key_index(0, 5), None);
    }

    #[test]
    fn key_validity_respects_count() {
        let m = Model::MK2;
        assert!(m.is_valid_key(0));
        assert!(m.is_valid_key(14));
        assert!(!m.is_valid_key(15));
    }
}
