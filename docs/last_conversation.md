> **Archived design sketch.** This was an exploratory conversation about how to
> handle key-and-parent nodes in the trie renderer. The question was resolved by
> the final `ViewRow` + `RowIdentity` design. Not a description of current code.

Let me map it through. The domain entries (real keys) are:

  app.testing
  com.myapp.error
  com.myapp.error.notfound
  com.myapp.error.servererror
  com.myapp.error.timeout
  com.myapp.error.timeout.deeper
  com.myapp.error.unauthorized
  com.myapp.error.unexpected

  com.myapp is a group header (non-key branch node, chain-collapsed from com →
  myapp). So the DrawBundle.entries list has 8 elements, one per real key.

  Entry: app.testing

  app has one child → chain collapses → no group header, just the key. One group:

  GroupedVec {
      data:    [app]  [testing]
      offsets: [0]
      //        └──── key group ────┘
      selected: Some(1)  // anchor at "testing"
  }
  prefix_groups() → empty. key_group() → [app, testing] displayed as app.testing.

  ---
  Entry: com.myapp.error

  com.myapp is a branch header (chain-collapsed, two segments). One prefix group,
  then the key:

  GroupedVec {
      data:    [com]  [myapp]  [error]
      offsets: [0,    2]
      //        └─ g0 ──┘      └─ g1 ─┘
      //           header         key
      selected: Some(2)  // anchor at "error"
  }
  prefix_groups() → [com, myapp] → rendered as com.myapp group header row.
  key_group() → [error] → rendered as .error key row.

  ---
  Entry: com.myapp.error.timeout.deeper

  Here I run into a question I want to ask you about. com.myapp.error and
  com.myapp.error.timeout are both key-and-parent nodes — they're already rendered
  as key rows by their own entries above. They're "context" for .deeper, but they're
   not group headers that this entry should render.

  Should those key-and-parent segments appear as their own groups in the GroupedVec
  (so the structure captures the full hierarchy), or do they get absorbed into the
  key group (keeping offsets only for actual group header rows)?

  Option A — full hierarchy in offsets, need a way to distinguish header groups from
   key-context groups:
  offsets: [0,    2,       3,        4]
  //        └─ g0 ─┘  └─ g1 ─┘ └─ g2 ─┘  └─ g3 ─┘
  //          header   key-ctx  key-ctx    key
  //         com.myapp  error   timeout   deeper

  Option B — offsets only for group header rows, key group absorbs the rest:
  offsets: [0,    2]
  //        └─ g0 ─┘  └──────── g1 (key group) ────────┘
  //          header    error · timeout · deeper

  Option B is simpler but the renderer loses the information that error and timeout
  are separate context levels within the key group. Option A preserves it but needs
  something extra — a header_count: usize field, or a GroupKind per group.

  Which way do you want to go?


I'd render it like this:

first entry:
app.testing
2 single groups -> we can just render it as normal entry

2. line
com.myapp.error
com.myapp is a group
error is a special case. it's a leaf after a group amd it also has children
we render it in two lines, the last group as header and then a normal entry

