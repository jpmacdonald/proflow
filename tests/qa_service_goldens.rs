//! Sanitized semantic evidence for the six operator-QA-approved service builds.

#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

#[derive(Deserialize)]
struct QaGoldens {
    schema: String,
    provenance: Provenance,
    services: Vec<ServiceGolden>,
}

#[derive(Deserialize)]
struct Provenance {
    status: String,
    producer_version: String,
    native_producer_version: String,
    operating_system: String,
    export_mode: String,
    sanitization: String,
}

#[derive(Deserialize)]
struct ServiceGolden {
    playlist_name: String,
    service_date: String,
    service_style: String,
    semantic_sha256: String,
    counts: ServiceCounts,
    covered_native_capabilities: BTreeSet<String>,
}

#[derive(Deserialize)]
struct ServiceCounts {
    review_entries: usize,
    playlist_items: usize,
    presentation_artifacts: usize,
    media_members: usize,
    generated: usize,
    edited: usize,
    restyled: usize,
    library: usize,
    skipped: usize,
}

#[test]
fn qa_service_goldens_cover_three_complete_service_weeks() {
    let goldens: QaGoldens =
        serde_json::from_str(include_str!("fixtures/workflow/qa-services.json"))
            .expect("QA golden corpus should parse");

    assert_eq!(goldens.schema, "proflow.qa-service-goldens.v1");
    assert_eq!(goldens.provenance.status, "operator_qa_approved");
    assert_eq!(goldens.provenance.export_mode, "portable_import");
    assert!(!goldens.provenance.producer_version.is_empty());
    assert!(!goldens.provenance.native_producer_version.is_empty());
    assert!(!goldens.provenance.operating_system.is_empty());
    assert!(goldens.provenance.sanitization.contains("UUID"));
    assert_eq!(goldens.services.len(), 6);

    let mut styles_by_date = BTreeMap::<&str, BTreeSet<&str>>::new();
    for service in &goldens.services {
        styles_by_date
            .entry(&service.service_date)
            .or_default()
            .insert(&service.service_style);
        assert!(service.playlist_name.contains("2026"));
        assert_eq!(service.semantic_sha256.len(), 64);
        assert!(service
            .semantic_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        assert_eq!(
            service.counts.review_entries,
            service.counts.playlist_items + service.counts.skipped
        );
        assert!(service.counts.presentation_artifacts <= service.counts.playlist_items);
        assert!(service.counts.media_members > 0);
        assert_eq!(
            service.counts.generated
                + service.counts.edited
                + service.counts.restyled
                + service.counts.library,
            service.counts.playlist_items
        );
        for capability in [
            "required_items",
            "scripture",
            "nametags",
            "responsive_liturgy",
            "graphics",
            "arrangements",
            "portable_media",
        ] {
            assert!(service.covered_native_capabilities.contains(capability));
        }
    }

    assert_eq!(styles_by_date.len(), 3);
    for styles in styles_by_date.values() {
        assert_eq!(styles, &BTreeSet::from(["contemporary", "traditional"]));
    }
}
