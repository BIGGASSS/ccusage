# pi-agent Source

Data source:

```text
${PI_AGENT_DIR:-~/.pi/agent/sessions/}
```

Usage accounting includes assistant requests made by pi's `subagent` tool. When
per-request assistant messages are available, ccusage uses their individual
models, timestamps, token counts, and costs; otherwise it falls back to the
subagent result's aggregate usage without counting both representations.

Commands:

```sh
ccusage pi daily
ccusage pi monthly
ccusage pi session
ccusage pi daily --json
ccusage pi daily --pi-path /path/to/sessions
```
