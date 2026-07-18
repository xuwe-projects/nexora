extern crate self as nexora;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NavigationGroupMetadata {
    id: &'static str,
    title: &'static str,
    section: &'static str,
    icon: Option<&'static str>,
    parent: Option<&'static str>,
    order: i32,
}

impl NavigationGroupMetadata {
    const fn new(
        id: &'static str,
        title: &'static str,
        section: &'static str,
        icon: Option<&'static str>,
        parent: Option<&'static str>,
        order: i32,
    ) -> Self {
        Self {
            id,
            title,
            section,
            icon,
            parent,
            order,
        }
    }
}

trait NavigationGroup: 'static {
    const METADATA: NavigationGroupMetadata;
}

mod __private {
    pub use inventory;

    use super::NavigationGroupMetadata;

    pub struct NavigationGroupRegistration {
        metadata: NavigationGroupMetadata,
    }

    impl NavigationGroupRegistration {
        pub const fn new(metadata: NavigationGroupMetadata) -> Self {
            Self { metadata }
        }

        pub const fn metadata(&self) -> NavigationGroupMetadata {
            self.metadata
        }
    }

    inventory::collect!(NavigationGroupRegistration);
}

#[derive(nexora_macros::NavigationGroup)]
#[nexora(
    id = "resources",
    title = "资料中心",
    section = "资料中心",
    icon = "folder",
    order = 10
)]
struct ResourcesGroup;

#[derive(nexora_macros::NavigationGroup)]
#[nexora(
    title = "生产建模",
    section = "资料中心",
    parent = "resources",
    order = 20
)]
struct ProductionModelNavigationGroup;

#[test]
fn navigation_group_derive_registers_recursive_directory_metadata() {
    assert_eq!(
        ResourcesGroup::METADATA,
        NavigationGroupMetadata {
            id: "resources",
            title: "资料中心",
            section: "资料中心",
            icon: Some("folder"),
            parent: None,
            order: 10,
        }
    );
    assert_eq!(
        ProductionModelNavigationGroup::METADATA,
        NavigationGroupMetadata {
            id: "production-model",
            title: "生产建模",
            section: "资料中心",
            icon: None,
            parent: Some("resources"),
            order: 20,
        }
    );

    let mut registrations = inventory::iter::<__private::NavigationGroupRegistration>
        .into_iter()
        .map(__private::NavigationGroupRegistration::metadata)
        .collect::<Vec<_>>();
    registrations.sort_by_key(|metadata| metadata.order);
    assert_eq!(registrations.len(), 2);
    assert_eq!(registrations[0].id, "resources");
    assert_eq!(registrations[1].parent, Some("resources"));
}
