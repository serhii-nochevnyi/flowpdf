use flow_core::layout::{FontCatalog, FontFace, LayoutError, LayoutUnit};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

#[test]
fn millipoints_and_font_units_use_checked_nearest_fixed_point_conversion() {
    assert_eq!(LayoutUnit::from_millipoints(12_000).unwrap().raw(), 768);
    assert_eq!(LayoutUnit::from_millipoints(1_000).unwrap().raw(), 64);
    assert_eq!(
        LayoutUnit::from_font_units(500, 1_000, LayoutUnit::from_raw(768))
            .unwrap()
            .raw(),
        384
    );
    assert_eq!(
        LayoutUnit::from_font_units(-500, 1_000, LayoutUnit::from_raw(768))
            .unwrap()
            .raw(),
        -384
    );
}

#[test]
fn fixed_point_addition_rejects_overflow() {
    let error = LayoutUnit::from_raw(i64::MAX)
        .checked_add(LayoutUnit::from_raw(1))
        .unwrap_err();
    assert_eq!(error, LayoutError::NumericOverflow);
    assert_eq!(error.code(), "FLOW_LAYOUT_NUMERIC_OVERFLOW");
}

#[test]
fn font_catalog_rejects_invalid_and_duplicate_faces() {
    assert_eq!(
        FontFace::new("invalid", "Invalid", 0, vec![1, 2, 3])
            .unwrap_err()
            .code(),
        "FLOW_LAYOUT_FONT_INVALID"
    );

    let first = FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap();
    let duplicate = FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap();
    assert_eq!(
        FontCatalog::new(vec![first, duplicate]).unwrap_err().code(),
        "FLOW_LAYOUT_FONT_DUPLICATE"
    );
}
