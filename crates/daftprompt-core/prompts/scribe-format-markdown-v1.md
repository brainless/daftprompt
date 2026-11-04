## Markdown format v1

A section whose heading names an option, refinement, or plan step is usually one
item: include the heading through its rationale and last bullet. A heading merely
grouping independent items stays unlabelled. Conditional refinements with a
Trigger and proposed change are still recommendations; label EVERY such sibling
section, even when the text defers execution until needed. Do not skip the earlier
refinements merely because later sections or bullets are easier to annotate. In bullet lists use whole item blocks,
including continuation lines and nested rationale. If nested bullets are independent
plan steps, select ALL those children instead of their grouping parent. A parent
such as "Prepare the rollout:" groups its independently executable children;
it is not one recommendation that absorbs those children. In contrast, a section's
Trigger / change / comparison bullets describe one conditional refinement.
A question that occupies a whole bullet item uses that item block, including its
bullet marker. Use a quote only for a question embedded inside a larger item.

Worked example:

```text
{b1 section 1-10}
[1] ## Improve if needed
[2]
  {b2 section 3-6}
[3] ### Retry with corrective feedback
[4]
    {b3 item 5-5}
[5] - Trigger: a high invalid-output rate.
    {b4 item 6-6}
[6] - Send the specific validation error on the next attempt.
  {b5 section 7-10}
[7] ### Add latency metrics
[8]
    {b6 item 9-10}
[9] - Record latency per model.
[10]   This helps compare configurations.
```

Submit:

```json
{
  "spans": [{"id":"s1","block":"b2"},{"id":"s2","block":"b5"}],
  "annotations": [
    {"kind":"recommendation","span":"s1"},
    {"kind":"recommendation","span":"s2"}
  ]
}
```

The section heading names the refinement. Its condition and implementation bullets
are parts of that same refinement, not separate recommendations. Select the SECTION
block in this situation, not each item below it. Apply this to every sibling section.
Do not select the grouping `b1`, isolate the trigger `b3`, or add child spans.
These proposed refinements are recommendations, not execution commitments.

Another example, independent nested steps:

```text
{b1 item 1-3}
[1] - Improve reliability:
  {b2 item 2-2}
[2]   - Add health checks. Alert on failures.
  {b3 item 3-3}
[3]   - Test recovery after a restart.
{b4 item 4-4}
[4] - Who owns the release?
```

Submit:

```json
{
  "spans": [
    {"id":"s1","block":"b2"},
    {"id":"s2","block":"b3"},
    {"id":"s3","block":"b4"}
  ],
  "annotations": [
    {"kind":"recommendation","span":"s1"},
    {"kind":"recommendation","span":"s2"},
    {"kind":"question","span":"s3"}
  ]
}
```

The parent bullet is only a group name; each child gives a separate plan step.
Leave `b1` unlabelled. This differs from a named refinement SECTION whose child
bullets explain one option. Never select both the parent and its children.

Coverage check before submit: a grouping heading can introduce optional, future,
low-priority or nice-to-have improvements. Those are still offered recommendations.
Annotate every independently actionable sibling bullet, including the last ones;
a bullet does not need to repeat "I recommend" when the heading establishes that
meaning. Do not stop after the first bullet or after processing named sections.

```text
{b1 section 1-3}
[1] ## Optional improvements
  {b2 item 2-2}
[2] - **Observability:** add tracing around provider calls.
  {b3 item 3-3}
[3] - **Packaging:** publish a reproducible installer.
```

```json
{
  "spans": [{"id":"s1","block":"b2"},{"id":"s2","block":"b3"}],
  "annotations": [
    {"kind":"recommendation","span":"s1"},
    {"kind":"recommendation","span":"s2"}
  ]
}
```

Leave the grouping section unlabelled; label both complete sibling items.

Headings describing roles (for example, an open-question section or an answer
section) only group their content: select the actual question/answer paragraph,
not its heading. A heading naming a proposed refinement is different: that name
and its explanation together constitute the recommendation. A recorded decision
is not a new recommendation merely because it has a heading.
