use updater::UpdateChannel;

#[test]
fn channel_installation_identity_keeps_stable_and_suffixes_prereleases() {
    assert_eq!(
        UpdateChannel::Stable.installation_id("com.example.product"),
        "com.example.product"
    );
    assert_eq!(
        UpdateChannel::Beta.installation_id("com.example.product"),
        "com.example.product.beta"
    );
    assert_eq!(
        UpdateChannel::Nightly.installation_id("com.example.product"),
        "com.example.product.nightly"
    );

    assert_eq!(UpdateChannel::Stable.display_name("Product"), "Product");
    assert_eq!(UpdateChannel::Beta.display_name("Product"), "Product Beta");
    assert_eq!(
        UpdateChannel::Nightly.display_name("Product"),
        "Product Nightly"
    );
}
