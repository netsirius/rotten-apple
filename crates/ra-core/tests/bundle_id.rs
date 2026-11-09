//! Un solo bundle ID (D-08, provisional). `ra-safety` lo usa para proteger
//! el log técnico (T-14) y `ra-core` para la carpeta de estado: si se cambia
//! en un sitio y no en el otro, la protección dejaría de cubrir el log.

#[test]
fn ra_core_and_ra_safety_share_the_bundle_id() {
    assert_eq!(ra_core::BUNDLE_ID, ra_safety::protected::BUNDLE_ID);
}
