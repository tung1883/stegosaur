//! The stop request is process-wide, so this file holds a single test and no other test binary shares it.
mod common;
use common::natural_image;
use stegosaur::app::hide::{run_hide, HideSpec, Layout, Naming, Payload};
use stegosaur::app::{AppConfig, Secret};
use stegosaur::cancel;
use stegosaur::crypto::KdfParams;
use stegosaur::Error;

#[test]
fn a_stop_request_ends_the_job_and_leaves_no_output() {
    let d = tempfile::tempdir().unwrap();
    let cover = d.path().join("a.png");
    natural_image(160, 120, 3).save(&cover).unwrap();
    let data = d.path().join("s.bin");
    std::fs::write(&data, vec![7u8; 3000]).unwrap();
    let spec = || HideSpec {
        layout: Layout::Single { payload: Payload::File(data.clone()), cover: cover.clone() },
        pass: Secret::new("pw"),
        cfg: AppConfig { kdf: KdfParams::fast(), ..AppConfig::default() },
        naming: Naming::Suffix,
        out_dir: None,
        overwrite: false,
        extra: false,
    };
    cancel::request();
    let e = run_hide(spec(), &mut |_| {}).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2, "nothing new was written");
    // after a reset the same job runs normally
    cancel::reset();
    assert!(run_hide(spec(), &mut |_| {}).is_ok());
}
