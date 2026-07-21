use super::super::*;

fn restyle_with_selector(selector: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "version": 4,
        "presentation_types": {
            "managed": {
                "kind": "graphic",
                "content_source": "static",
                "output_strategy": "restyle_existing",
                "operator_cue_limit": 1,
                "macro_transitions": {
                    "regions": [{
                        "selector": selector,
                        "enter_macro": "Graphics"
                    }]
                }
            }
        }
    })
}

#[test]
fn operator_macro_selector_must_survive_the_configured_cue_limit() {
    let error = parse_project_config_value(restyle_with_selector(&serde_json::json!({
        "kind": "operator_cue",
        "index": 1
    })))
    .expect_err("a macro cannot target a cue removed before macro enforcement");
    let message = error.to_string();
    assert!(
        message.contains("presentation_types.managed.macro_transitions.regions.0.selector.index")
    );
    assert!(message.contains("operator cue index 1 is not retained by operator_cue_limit 1"));

    parse_project_config_value(restyle_with_selector(&serde_json::json!({
        "kind": "operator_cue",
        "index": 0
    })))
    .expect("the last retained operator cue remains a valid macro target");
}

#[test]
fn cue_limit_does_not_bound_native_arrangement_group_selectors() {
    parse_project_config_value(restyle_with_selector(&serde_json::json!({
        "kind": "arrangement_group",
        "index": 7,
        "names": ["Verse"]
    })))
    .expect("arrangement selectors are validated against native structure at execution");
}

#[test]
fn restyle_macro_regions_cannot_repeat_one_selector() {
    let config = serde_json::json!({
        "version": 4,
        "presentation_types": {
            "managed": {
                "kind": "graphic",
                "content_source": "static",
                "output_strategy": "restyle_existing",
                "macro_transitions": {
                    "regions": [
                        {
                            "selector": { "kind": "operator_cue", "index": 0 },
                            "enter_macro": "Graphics"
                        },
                        {
                            "selector": { "kind": "operator_cue", "index": 0 },
                            "enter_macro": "Graphics"
                        }
                    ]
                }
            }
        }
    });

    let error = parse_project_config_value(config)
        .expect_err("one native macro target cannot have two configured owners");

    assert!(error
        .to_string()
        .contains("selects operator cue 0 more than once"));
}
