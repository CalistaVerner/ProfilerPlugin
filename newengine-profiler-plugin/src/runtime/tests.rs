use super::*;

#[test]
fn duration_parser_reads_first_millisecond_pair_without_token_vector() {
    assert_eq!(
        parse_first_ms_from_text("load took 12.5 ms total"),
        Some(12.5)
    );
    assert_eq!(
        parse_first_ms_from_text("gpu 7 milliseconds blocked"),
        Some(7.0)
    );
    assert_eq!(parse_first_ms_from_text("no duration"), None);
}

#[test]
fn classification_join_is_lowercase_and_space_stable() {
    assert_eq!(
        lower_join(&[Some("RUNNING"), None, Some("GPU Wait")]),
        "running gpu wait"
    );
}

#[test]
fn breakdown_parser_keeps_valid_non_negative_parts() {
    assert_eq!(
        parse_breakdown_parts("decode=1.5ms upload=2ms ignored broken=x"),
        vec![("decode".to_owned(), 1.5), ("upload".to_owned(), 2.0)]
    );
}

#[test]
fn bool_metadata_parser_is_case_insensitive_without_normalization() {
    let value = json!({"enabled": "YeS", "disabled": "OFF"});
    assert_eq!(first_value_bool(&value, &["/enabled"]), Some(true));
    assert_eq!(first_value_bool(&value, &["/disabled"]), Some(false));
}
