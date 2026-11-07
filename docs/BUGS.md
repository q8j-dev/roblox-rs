# Bugs fixed during the port

| # | Legacy location | Bug | Fix | Regression test |
|---|---|---|---|---|
| 1 | `App/v8xml/XmlSerializer.cpp` (TextXmlParser::readText, TextXmlWriter) | Leading whitespace in single-line strings was lost on save and reload, because the reader skips whitespace before text and the writer only uses CDATA for multi-line text | The writer encodes leading whitespace as numeric entities | `rbx-format` `xml::tests::keeps_leading_whitespace` |
| 2 | `App/v8xml/XmlSerializer.cpp` (TextXmlParser) | A missing child element (for example a Vector3 without `<Z>`) dereferenced a null pointer | Missing components read as 0 | covered by `rbx-format` place round-trip test |
| 3 | `content/fonts/characterR15.rbxm` | File damaged by LF to CRLF conversion before it was committed | Rebuilt by trying each ambiguous CR LF pair until every chunk decompresses and the chunk chain is complete | `rbx-format` `tests/places.rs` |
