use pnpm_guru::{DEFAULT_REGISTRY, RegistryClient};

#[test]
fn live_registry_packument_and_tarball() {
    if std::env::var("PG_NET").as_deref() != Ok("1") {
        return;
    }
    let client = RegistryClient::new(DEFAULT_REGISTRY);
    let index = client.packument_index("left-pad").unwrap();
    assert!(index.package_count() >= 1);
    let entry = index.entry("left-pad", "1.3.0").expect("1.3.0 exists");
    assert!(
        entry
            .integrity
            .as_deref()
            .is_some_and(|i| i.starts_with("sha512-"))
    );
    let tarball_url = entry.tarball.clone().unwrap();
    let bytes = client.tarball(&tarball_url).unwrap();
    assert!(bytes.starts_with(&[0x1f, 0x8b]));
    let store = pnpm_guru::ContentStore::new(
        std::env::temp_dir().join(format!("pnpm-guru-net-{}", std::process::id())),
    );
    let unpacked = pnpm_guru::unpack_tarball(
        &store,
        &bytes,
        entry.integrity.as_deref(),
        "left-pad",
        "1.3.0",
    )
    .unwrap();
    assert_eq!(unpacked.manifest.name, "left-pad");
    assert_eq!(unpacked.manifest.version, "1.3.0");
    assert!(unpacked.files > 0);
}
