# Knowledge documentation

This directory documents the **Knowledge subsystem**. It does not contain a user's actual runtime Knowledge.

Runtime Knowledge lives in an initialized mail repository:

```text
knowledge/
├── global/
└── accounts/<account-uuid>/
```

Knowledge is user-approved semantic preference data used by AI harnesses to make better triage judgements. See [`../design/knowledge-system.md`](../design/knowledge-system.md).
