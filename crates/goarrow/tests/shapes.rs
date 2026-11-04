//! The smooth shapes the arrow and the map draw: coverage full inside,
//! none outside, partial along the edges, and the arrow symmetric.

use goarrow::shapes::{self, Mask, SAMPLES};

/// The arrow at 100 x 100, 44 pixels from its middle to its tip.
fn arrow() -> Mask {
    shapes::arrow(100, 44.0, 0.0)
}

fn partial(a: u8) -> bool {
    a > 0 && a < 255
}

#[test]
fn a_shape_is_full_inside_empty_outside_and_partial_along_its_edges() {
    let m = arrow();
    assert_eq!((m.width, m.height, m.alpha.len()), (100, 100, 10_000));
    assert_eq!(m.at(50, 50), 255, "the middle");
    assert_eq!(m.at(49, 20), 255, "toward the tip");
    for (x, y) in [(0, 0), (99, 0), (0, 99), (99, 99), (50, 2), (50, 80)] {
        assert_eq!(
            m.at(x, y),
            0,
            "({x}, {y}): outside (above the tip, in the notch)"
        );
    }
    // Across the middle row: nothing, then the edge, then all.
    let row: Vec<u8> = (50..100).map(|x| m.at(x, 50)).collect();
    let edge = row.iter().position(|a| *a < 255).unwrap();
    assert!(partial(row[edge]), "{row:?}");
    assert!(row[edge + 1..].iter().all(|a| *a < row[edge]), "{row:?}");
    assert!(m.alpha.iter().filter(|a| partial(**a)).count() > 100);
    // Coverage counts the samples a pixel's area holds.
    let steps = SAMPLES * SAMPLES;
    let levels: Vec<u8> = (0..=steps)
        .map(|k| ((k * 255 + steps / 2) / steps) as u8)
        .collect();
    assert!(m.alpha.iter().all(|a| levels.contains(a)));
}

#[test]
fn the_arrow_points_up_and_is_left_right_symmetric() {
    let m = arrow();
    for y in 0..100 {
        for x in 0..50 {
            assert_eq!(m.at(x, y), m.at(99 - x, y), "({x}, {y})");
        }
    }
    let width = |y: u32| (0..100).filter(|x| m.at(*x, y) > 0).count();
    assert!(
        width(12) < width(40),
        "narrow at the tip, wide toward the wings"
    );
    assert!(m.at(50, 7) > 0 && m.at(50, 5) == 0, "the tip 44 px up");
    assert!(m.at(19, 85) > 0, "a wing's end 31 px left, 36 px down");
    // Its lit face is its left half.
    let lit = shapes::arrow_left_half(100, 44.0);
    assert_eq!(lit.at(45, 50), 255);
    for y in 0..100 {
        for x in 0..100 {
            assert!(lit.at(x, y) <= m.at(x, y), "({x}, {y})");
            if x >= 50 {
                assert_eq!(lit.at(x, y), 0, "({x}, {y})");
            }
        }
    }
}

#[test]
fn a_grown_shape_covers_the_shape_and_a_margin_round_it() {
    let m = arrow();
    let grown = shapes::arrow(100, 44.0, 3.5);
    assert!(grown.alpha.iter().zip(&m.alpha).all(|(g, a)| g >= a));
    assert_eq!(m.at(50, 3), 0);
    assert!(grown.at(50, 3) > 0, "past the tip");
    assert!(grown.at(50, 80) == 0, "the notch stays open");
    for y in 0..100 {
        for x in 0..50 {
            assert_eq!(grown.at(x, y), grown.at(99 - x, y), "({x}, {y})");
        }
    }
}

#[test]
fn the_dial_is_a_smooth_ring_with_four_ticks_north_the_strongest() {
    let ring = shapes::ring(152, 58.0, 3.0);
    assert_eq!(ring.at(76, 76), 0, "hollow");
    assert_eq!(ring.at(134, 76), 255, "on the band");
    assert!(partial(ring.at(132, 76)), "its inner edge");
    assert_eq!(ring.at(0, 0), 0);
    assert!(ring.alpha.iter().filter(|a| partial(**a)).count() > 200);
    let ticks = shapes::ticks(152, 62.0, 68.0, 4.0, (61.0, 72.0, 6.0));
    assert_eq!(ticks.at(76, 10), 255, "north");
    assert_eq!(ticks.at(140, 76), 255, "east");
    assert_eq!(ticks.at(76, 142), 255, "south");
    assert_eq!(ticks.at(11, 76), 255, "west");
    assert_eq!(ticks.at(76, 76), 0);
    assert!(ticks.at(76, 5) > 0, "north reaches further");
    assert_eq!(ticks.at(76, 146), 0, "south does not");
    let wide = |y: u32| (0..152).filter(|x| ticks.at(*x, y) > 0).count();
    let tall = |x: u32| (0..152).filter(|y| ticks.at(x, *y) > 0).count();
    assert!(wide(10) > tall(140), "north is wider");
    let hub = shapes::disk(16, 5.0);
    assert_eq!((hub.at(8, 8), hub.at(0, 0)), (255, 0));
}

#[test]
fn the_pin_has_a_round_head_and_a_point() {
    let pin = shapes::pin(36, 52, 16.0, 12.0, 47.0, 0.0);
    assert_eq!(pin.at(18, 16), 255, "the head");
    assert_eq!(pin.at(17, 40), 255, "narrowing");
    assert!(pin.at(18, 46) > 0, "the point");
    assert_eq!(pin.at(18, 49), 0, "below it");
    for (x, y) in [(0, 0), (35, 0), (0, 51), (35, 51), (4, 40)] {
        assert_eq!(pin.at(x, y), 0, "({x}, {y})");
    }
    for y in 0..52 {
        for x in 0..18 {
            assert_eq!(pin.at(x, y), pin.at(35 - x, y), "({x}, {y})");
        }
    }
    assert!(pin.alpha.iter().filter(|a| partial(**a)).count() > 40);
    let edge = shapes::pin(36, 52, 16.0, 12.0, 47.0, 2.0);
    assert!(edge.alpha.iter().zip(&pin.alpha).all(|(e, p)| e >= p));
    assert!(edge.at(18, 48) > 0 && pin.at(18, 48) == 0, "past the point");
}

#[test]
fn a_mask_is_a_white_image_its_coverage_the_alpha() {
    let m = Mask::rasterise(3, 1, |x, _| x < 1.5);
    assert_eq!(m.alpha, [255, 128, 0]);
    assert_eq!(
        m.rgba(),
        [255, 255, 255, 255, 255, 255, 255, 128, 255, 255, 255, 0]
    );
}
