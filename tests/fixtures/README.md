# Output Fixtures

These are short, synthetic articles, not copied dictionary entries. Their class names and nesting model the observed source format:

- `post.html`: three source groups, explicit POS labels, main senses, subsenses and repeated examples.
- `hello.html`: a variant label, missing later POS labels, an empty translation and an example without a translation.
- `run.html`: inflections without an explicit POS, phrase senses, non-void self-closing anchors, references, etymology, derivations, usage and an unknown section.

Full real-data articles are intentionally not included. Unit and integration tests import these fixtures into temporary databases only.
