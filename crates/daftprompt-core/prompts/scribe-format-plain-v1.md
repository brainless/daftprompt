## Plain format v1

A paragraph is one recommendation only when the entire paragraph discusses ONE
proposal and its rationale/conditions. Paragraph blocks may instead contain
several independent items, even without blank lines. Then select each item by
segment range or exact quote; never select the parent paragraph for all of them.
Questions and tasks in prose are individual sentences. Keep standalone question
and answer line prefixes such as `Q:` and `A:` when they belong to the source item.
For an email's `> ` quotation marker, quote the actual question without the marker.

A suggested plan's separate steps stay separate recommendations. An introduction
such as "Suggested plan:" and an explicitly rejected option stay unlabelled.
An observation that an owner is needed is background, not an outstanding task.
Plain-text list-looking lines have no special Markdown structure.

Example: one paragraph, several items, no blank lines:

```text
{b1 paragraph 1-3}
[1] Which storage should we use?
[2] I suggest SQLite. It avoids a separate database service.
[3] @lee, please benchmark write throughput.
```

```json
{
  "spans": [
    {"id":"s1","start_segment":1,"end_segment":1},
    {"id":"s2","start_segment":2,"end_segment":2},
    {"id":"s3","start_segment":3,"end_segment":3}
  ],
  "annotations": [
    {"kind":"question","span":"s1"},
    {"kind":"recommendation","span":"s2","questions":["s1"]},
    {"kind":"task","span":"s3","assignee":"@lee"}
  ]
}
```

Never select `b1` here: the question, suggestion and task are different items.
Do not add an answer: suggesting an option does not decide the question.

A wrapped question occupies one item, even over several segments:

```text
{b1 paragraph 1-2}
[1] Should we ship this quarter, or wait for the database
[2] migration to finish?
```

Submit `{"id":"s1","block":"b1"}` with a question annotation. Quoting only
`migration to finish?` loses the question's meaning. Include the whole sentence.

Final semantic check:
- Descriptive context such as "we are considering a change" is not itself an
  offered recommendation. A paragraph introducing the situation or explaining
  prerequisites before a decision question can stay unlabelled.
- An explicitly rejected option is not offered advice. In a suggested plan, a
  sentence such as "Do not choose an unbounded queue: it exhausts memory" stays
  unlabelled, while the positively offered steps each receive recommendations.
- Include the complete question across physical line wraps, without absorbing
  a separate background paragraph into it.

When the text is JSON or a similar data envelope, quote the complete question or
answer VALUE itself. Property names, outer string delimiters and trailing commas
are framing, not part of the question/answer. Other descriptive properties stay
unlabelled unless they independently contain an actual request or offered option.
