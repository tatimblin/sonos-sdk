//! Integration tests for DIDL-Lite parsing functionality

use sonos_parser::{DidlLite, DidlItem, DidlResource};

#[test]
fn test_didl_lite_top_level_import() {
    let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="-1" parentID="-1"><dc:title>Integration Test Song</dc:title><dc:creator>Integration Test Artist</dc:creator><upnp:album>Integration Test Album</upnp:album></item></DIDL-Lite>"#;
    
    let result = DidlLite::from_xml(didl_xml);
    assert!(result.is_ok(), "Failed to parse DIDL-Lite from top-level import: {:?}", result.err());
    
    let didl = result.unwrap();
    assert_eq!(didl.item.title, Some("Integration Test Song".to_string()));
    assert_eq!(didl.item.creator, Some("Integration Test Artist".to_string()));
    assert_eq!(didl.item.album, Some("Integration Test Album".to_string()));
}

#[test]
fn test_didl_structures_accessible() {
    // Test that all DIDL-Lite structures are accessible from top-level
    let _didl_lite: DidlLite;
    let _didl_item: DidlItem;
    let _didl_resource: DidlResource;
    
    // This test passes if it compiles, proving the types are accessible
}