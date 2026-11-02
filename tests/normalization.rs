use whorl::similarity::normalize::NormalizationVersion;

#[test]
fn golden_normalization_vectors() {
    let cases = [
        ("CAFÉ cafe\u{301}", "café café"),
        ("Ｆｕｌｌ ① ﬃ ²", "full 1 ffi 2"),
        ("Don't co-operate foo_bar a/b", "dont cooperate foobar ab"),
        ("“Hello,”—world…!", "helloworld"),
        (
            " \t One\r\n\u{a0}Two\u{2003}Three \u{2028}",
            "one two three",
        ),
        ("İ I ı ß ẞ Σ σ ς ΟΣ", "i\u{307} i ı ß ß σ σ ς οσ"),
        ("हिन्दी العربية שָׁלוֹם", "हिन्दी العربية שָׁלוֹם"),
        ("É e E", "é e e"),
        ("A-\u{301}", "á"),
        ("a💡+=$b\u{200b}c\0", "abc"),
        ("Header\nBody\nHeader", "header body header"),
        ("word-\nwrap", "word wrap"),
        ("", ""),
        (" \t\r\n", ""),
        ("...💡+\0", ""),
    ];
    for (input, expected) in cases {
        let actual = NormalizationVersion::V1.normalize(input);
        assert_eq!(actual, expected, "input: {input:?}");
        assert_eq!(NormalizationVersion::V1.normalize(&actual), actual);
    }
}

#[test]
fn version_one_unicode_tables_are_fixed() {
    assert_eq!(char::UNICODE_VERSION, (17, 0, 0));
    assert_eq!(unicode_normalization::UNICODE_VERSION, (18, 0, 0));
}
