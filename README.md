# ACP adapter for Codex

Use [Codex](https://github.com/openai/codex) from [ACP-compatible](https://agentclientprotocol.com) clients such as [Zed](https://zed.dev)!

Russian README: [README.ru.md](README.ru.md)

This tool implements an ACP adapter around the Codex CLI, supporting:

- Context @-mentions
- Images
- Tool calls (with permission requests)
- Session listing and resume
- History replay on load/resume
- Model + reasoning effort config options
- Context usage updates (tokens/window) for ACP clients that support it
- Slash commands:
  - /threads
  - /resume <thread_id>
  - /compact
  - /undo [num_turns]
  - /reasoning [none|minimal|low|medium|high|xhigh]
  - /plan [on|off|plan|default|<request>]
  - /context
- Auth methods:
  - ChatGPT subscription
  - CODEX_API_KEY
  - OPENAI_API_KEY

Current limitation:

- Client-provided MCP servers are accepted by ACP but not yet forwarded into Codex app-server mode.

Dev logging tip:

- Set `CODEX_ACP_DEV_LOGS_WITHOUT_TEXT_OUTPUT=1` to suppress text chunk updates (`agent_message_chunk`, `agent_thought_chunk`, `user_message_chunk`) in ACP traffic while keeping tool calls, diffs, and plan updates. This is useful when inspecting ACP logs with less token-stream noise.

Local CAS workflow:

- Run automated checks:
  - Quick: `bash script/run_live_checks.sh quick`
  - Full: `bash script/run_live_checks.sh full`
- Build + install + smoke-test in one step:
  - Default install: `bash script/build_install_cas.sh`
  - With checks before build: `bash script/build_install_cas.sh --with-checks --checks-mode quick`
  - Full checks: `bash script/build_install_cas.sh --with-checks --checks-mode full`
  - Custom destination/name: `bash script/build_install_cas.sh "$HOME/bin" codex-acp-cas`
  - Skip smoke test (not recommended): `bash script/build_install_cas.sh --no-smoke-test`
- Standalone smoke test for any installed binary:
  - `bash script/smoke_test_cas.sh "$HOME/.local/bin/codex-acp-cas"`
- Each install writes `*.build-info.txt` with version, commit, dirty flag, sha256, rustc, and build timestamp.

Versioned release workflow:

- CAS uses an independent SemVer scheme (not tied to upstream codex-acp tags), e.g. `0.1.0`, `0.1.1`.
- Prepare a versioned release commit/tag locally:
  - `bash script/prepare_release.sh 0.1.0`
  - optional full checks: `bash script/prepare_release.sh 0.1.1 --checks-mode full`
  - dry prep without tag/build: `bash script/prepare_release.sh 0.2.0-rc.1 --no-tag --no-build --checks-mode none`
- Push to trigger the release pipeline:
  - `git push origin main`
  - `git push origin v0.1.0`
- GitHub release automation builds Linux (`x86_64-unknown-linux-gnu`) artifacts only.

Learn more about the [Agent Client Protocol](https://agentclientprotocol.com/).

## How to use

### Zed

The latest version of Zed can already use this adapter out of the box.

To use Codex, open the Agent Panel and click "New Codex Thread" from the `+` button menu in the top-right.

Read the docs on [External Agent](https://zed.dev/docs/ai/external-agents) support.

### Other clients

Or try it with any of the other [ACP compatible clients](https://agentclientprotocol.com/overview/clients)!

#### Installation

Install the adapter from the latest release for your architecture and OS: https://github.com/zed-industries/codex-acp/releases

You can then use `codex-acp` as a regular ACP agent:

```
OPENAI_API_KEY=sk-... codex-acp
```

Or via npm:

```
npx @zed-industries/codex-acp
```

## License

Apache-2.0
