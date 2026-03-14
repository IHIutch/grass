//! A big dictionary of units and their conversion ratios.
//!
//! Arbitrary precision is retained.

use std::{f64::consts::PI, iter::FromIterator};

use rustc_hash::{FxHashMap, FxHashSet};

use std::sync::LazyLock;

use crate::unit::Unit;

pub(crate) static UNIT_CONVERSION_TABLE: LazyLock<FxHashMap<Unit, FxHashMap<Unit, f64>>> =
    LazyLock::new(|| {
        let mut from_in = FxHashMap::default();
        from_in.insert(Unit::In, 1.0);
        from_in.insert(Unit::Cm, 1.0 / 2.54);
        from_in.insert(Unit::Pc, 1.0 / 6.0);
        from_in.insert(Unit::Mm, 1.0 / 25.4);
        from_in.insert(Unit::Q, 1.0 / 101.6);
        from_in.insert(Unit::Pt, 1.0 / 72.0);
        from_in.insert(Unit::Px, 1.0 / 96.0);

        let mut from_cm = FxHashMap::default();
        from_cm.insert(Unit::In, 2.54);
        from_cm.insert(Unit::Cm, 1.0);
        from_cm.insert(Unit::Pc, 2.54 / 6.0);
        from_cm.insert(Unit::Mm, 1.0 / 10.0);
        from_cm.insert(Unit::Q, 1.0 / 40.0);
        from_cm.insert(Unit::Pt, 2.54 / 72.0);
        from_cm.insert(Unit::Px, 2.54 / 96.0);

        let mut from_pc = FxHashMap::default();
        from_pc.insert(Unit::In, 6.0);
        from_pc.insert(Unit::Cm, 6.0 / 2.54);
        from_pc.insert(Unit::Pc, 1.0);
        from_pc.insert(Unit::Mm, 6.0 / 25.4);
        from_pc.insert(Unit::Q, 6.0 / 101.6);
        from_pc.insert(Unit::Pt, 1.0 / 12.0);
        from_pc.insert(Unit::Px, 1.0 / 16.0);

        let mut from_mm = FxHashMap::default();
        from_mm.insert(Unit::In, 25.4);
        from_mm.insert(Unit::Cm, 10.0);
        from_mm.insert(Unit::Pc, 25.4 / 6.0);
        from_mm.insert(Unit::Mm, 1.0);
        from_mm.insert(Unit::Q, 1.0 / 4.0);
        from_mm.insert(Unit::Pt, 25.4 / 72.0);
        from_mm.insert(Unit::Px, 25.4 / 96.0);

        let mut from_q = FxHashMap::default();
        from_q.insert(Unit::In, 101.6);
        from_q.insert(Unit::Cm, 40.0);
        from_q.insert(Unit::Pc, 101.6 / 6.0);
        from_q.insert(Unit::Mm, 4.0);
        from_q.insert(Unit::Q, 1.0);
        from_q.insert(Unit::Pt, 101.6 / 72.0);
        from_q.insert(Unit::Px, 101.6 / 96.0);

        let mut from_pt = FxHashMap::default();
        from_pt.insert(Unit::In, 72.0);
        from_pt.insert(Unit::Cm, 72.0 / 2.54);
        from_pt.insert(Unit::Pc, 12.0);
        from_pt.insert(Unit::Mm, 72.0 / 25.4);
        from_pt.insert(Unit::Q, 72.0 / 101.6);
        from_pt.insert(Unit::Pt, 1.0);
        from_pt.insert(Unit::Px, 3.0 / 4.0);

        let mut from_px = FxHashMap::default();
        from_px.insert(Unit::In, 96.0);
        from_px.insert(Unit::Cm, 96.0 / 2.54);
        from_px.insert(Unit::Pc, 16.0);
        from_px.insert(Unit::Mm, 96.0 / 25.4);
        from_px.insert(Unit::Q, 96.0 / 101.6);
        from_px.insert(Unit::Pt, 4.0 / 3.0);
        from_px.insert(Unit::Px, 1.0);

        let mut from_deg = FxHashMap::default();
        from_deg.insert(Unit::Deg, 1.0);
        from_deg.insert(Unit::Grad, 9.0 / 10.0);
        from_deg.insert(Unit::Rad, 180.0 / PI);
        from_deg.insert(Unit::Turn, 360.0);

        let mut from_grad = FxHashMap::default();
        from_grad.insert(Unit::Deg, 10.0 / 9.0);
        from_grad.insert(Unit::Grad, 1.0);
        from_grad.insert(Unit::Rad, 200.0 / PI);
        from_grad.insert(Unit::Turn, 400.0);

        let mut from_rad = FxHashMap::default();
        from_rad.insert(Unit::Deg, PI / 180.0);
        from_rad.insert(Unit::Grad, PI / 200.0);
        from_rad.insert(Unit::Rad, 1.0);
        from_rad.insert(Unit::Turn, 2.0 * PI);

        let mut from_turn = FxHashMap::default();
        from_turn.insert(Unit::Deg, 1.0 / 360.0);
        from_turn.insert(Unit::Grad, 1.0 / 400.0);
        from_turn.insert(Unit::Rad, 1.0 / (2.0 * PI));
        from_turn.insert(Unit::Turn, 1.0);

        let mut from_s = FxHashMap::default();
        from_s.insert(Unit::S, 1.0);
        from_s.insert(Unit::Ms, 1.0 / 1000.0);

        let mut from_ms = FxHashMap::default();
        from_ms.insert(Unit::S, 1000.0);
        from_ms.insert(Unit::Ms, 1.0);

        let mut from_hz = FxHashMap::default();
        from_hz.insert(Unit::Hz, 1.0);
        from_hz.insert(Unit::Khz, 1000.0);

        let mut from_khz = FxHashMap::default();
        from_khz.insert(Unit::Hz, 1.0 / 1000.0);
        from_khz.insert(Unit::Khz, 1.0);

        let mut from_dpi = FxHashMap::default();
        from_dpi.insert(Unit::Dpi, 1.0);
        from_dpi.insert(Unit::Dpcm, 2.54);
        from_dpi.insert(Unit::Dppx, 96.0);

        let mut from_dpcm = FxHashMap::default();
        from_dpcm.insert(Unit::Dpi, 1.0 / 2.54);
        from_dpcm.insert(Unit::Dpcm, 1.0);
        from_dpcm.insert(Unit::Dppx, 96.0 / 2.54);

        let mut from_dppx = FxHashMap::default();
        from_dppx.insert(Unit::Dpi, 1.0 / 96.0);
        from_dppx.insert(Unit::Dpcm, 2.54 / 96.0);
        from_dppx.insert(Unit::Dppx, 1.0);

        let mut m = FxHashMap::default();
        m.insert(Unit::In, from_in);
        m.insert(Unit::Cm, from_cm);
        m.insert(Unit::Pc, from_pc);
        m.insert(Unit::Mm, from_mm);
        m.insert(Unit::Q, from_q);
        m.insert(Unit::Pt, from_pt);
        m.insert(Unit::Px, from_px);

        m.insert(Unit::Deg, from_deg);
        m.insert(Unit::Grad, from_grad);
        m.insert(Unit::Rad, from_rad);
        m.insert(Unit::Turn, from_turn);

        m.insert(Unit::S, from_s);
        m.insert(Unit::Ms, from_ms);

        m.insert(Unit::Hz, from_hz);
        m.insert(Unit::Khz, from_khz);

        m.insert(Unit::Dpi, from_dpi);
        m.insert(Unit::Dpcm, from_dpcm);
        m.insert(Unit::Dppx, from_dppx);

        m
    });

pub(crate) static KNOWN_COMPATIBILITIES: LazyLock<[FxHashSet<Unit>; 5]> = LazyLock::new(|| {
    let dimensions = FxHashSet::from_iter([
        Unit::Em,
        Unit::Ex,
        Unit::Ch,
        Unit::Rem,
        Unit::Vw,
        Unit::Vh,
        Unit::Vmin,
        Unit::Vmax,
        Unit::Cm,
        Unit::Mm,
        Unit::Q,
        Unit::In,
        Unit::Pt,
        Unit::Pc,
        Unit::Px,
    ]);
    let angles = FxHashSet::from_iter([Unit::Deg, Unit::Grad, Unit::Rad, Unit::Turn]);
    let time = FxHashSet::from_iter([Unit::S, Unit::Ms]);
    let frequency = FxHashSet::from_iter([Unit::Hz, Unit::Khz]);
    let resolution = FxHashSet::from_iter([Unit::Dpi, Unit::Dpcm, Unit::Dppx]);

    [dimensions, angles, time, frequency, resolution]
});

pub(crate) fn known_compatibilities_by_unit(unit: &Unit) -> Option<&FxHashSet<Unit>> {
    match unit {
        Unit::Em
        | Unit::Ex
        | Unit::Ch
        | Unit::Rem
        | Unit::Vw
        | Unit::Vh
        | Unit::Vmin
        | Unit::Vmax
        | Unit::Cm
        | Unit::Mm
        | Unit::Q
        | Unit::In
        | Unit::Pt
        | Unit::Pc
        | Unit::Px => Some(&KNOWN_COMPATIBILITIES[0]),
        Unit::Deg | Unit::Grad | Unit::Rad | Unit::Turn => Some(&KNOWN_COMPATIBILITIES[1]),
        Unit::S | Unit::Ms => Some(&KNOWN_COMPATIBILITIES[2]),
        Unit::Hz | Unit::Khz => Some(&KNOWN_COMPATIBILITIES[3]),
        Unit::Dpi | Unit::Dpcm | Unit::Dppx => Some(&KNOWN_COMPATIBILITIES[4]),
        _ => None,
    }
}
