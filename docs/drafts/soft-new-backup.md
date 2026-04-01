# `soft /new` backup

Status: removed from surfaced slash UX on `2026-04-02`.

Why it was removed:
- `Zed` already has a native `New Thread` action that creates a real new ACP session.
- Slash `/new` only switched the backend thread inside the current ACP session.
- That in-place switch could not clear the current chat transcript or sidebar state, so it looked like a partial duplicate of `Zed`'s native action with worse UX.

What the old flow did:
- Called `thread/start` with the current session settings.
- Rebound the current ACP session to the fresh backend thread.
- Reset runtime state such as usage/context counters.
- Left existing ACP chat UI history visible, because the client did not reset the session view.

Why the code shape still exists indirectly:
- Archive of the current thread still needs to start a fresh replacement backend thread.
- That internal helper remains useful for archive-triggered replacement, even though `/new` is no longer surfaced as a user command.

If this is revisited later:
1. Prefer a client-native path first.
2. Only bring slash `/new` back if the ACP client can clearly communicate that it is an in-place backend switch rather than a real new chat.
3. Keep `Zed` `New Thread` as the canonical path for a clean new ACP session.
