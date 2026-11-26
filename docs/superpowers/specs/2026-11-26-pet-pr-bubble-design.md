# Clicking a PR bubble opens the PR

Step 2 of the Bichinho roadmap (an assistant, not a Tamagotchi): when the pet says a PR needs a fix or got approved, a click on its bubble opens that PR in the browser. Today the click only closes the bubble, and an extension cannot open a URL.

## Goals

- A `pr_fix` or `pr_approved` bubble carries its PR's URL; clicking it opens the URL and closes the bubble.
- PR bubbles stay until clicked (sticky), so the click is not a race against a 4 s timeout.
- A PR bubble goes away by itself when its PR leaves that state (fixed, merged, closed).

Non-goals: opening a session from a bubble, listing PRs in the card, telling apart several PRs in the text.

Anti-goal: a sticky PR bubble must never hide a `needs_you`.

## Approaches

- **`url` on the `bubble` node (picked).** wai opens it on the user's click, as `link` does. No new host call, no permission, no rate limit: only a click opens anything.
- A `url/open` host call. More flexible (step 5 might want it), but an extension could open URLs with no click, so it needs a permission and a rate limit. Not needed yet.

## wai (API 1.10)

- `bubble` takes `url`?: string, validated like `link` (`web_url`: http/https, at most 2,048 characters). Present but invalid makes the node unsupported, as with `link`.
- With `url`: the click calls `cx.open_url(url)` and then ends the bubble as today (`panel/action` with `{"bubble": "click"}`). Pointer cursor; tooltip shows the URL. Clicking the collapsed "!" of a sticky bubble opens it too.
- `API` goes to `"1.10"`; `docs/extensions.md` documents `url` and the version.
- Released first (a `feat:` commit), so the pet can require it.

## wai-bichinho 0.6.0

- `line` gets `URL`; the dock sends it as the bubble's `url`. `pr_fix` and `pr_approved` become sticky.
- `gotPRs` passes the URL: the first PR in `prs.mine`'s order that changed into the line's state; on the first event, the first PR that needs a fix.
- Speech:
  - Dedup compares text and URL, so a second PR with the same line still queues.
  - A priority 3 line (`needs_you`, `error`) preempts a sticky lower-priority line too; the preempted line goes back to the head of its priority in the queue and shows again later.
  - After each `prs.mine`, PR lines (shown or queued) whose PR left the line's state are dropped: `pr_fix` while not `attention`, `pr_approved` while not `approved`.
- Manifest `api = "1.10"`, README updated, released on its own `wai-bichinho-v0.6.0` tag and listed in the index (`chore:` commits).

## Left to the builder

Test layout, and how the dock passes the URL into the tree.
