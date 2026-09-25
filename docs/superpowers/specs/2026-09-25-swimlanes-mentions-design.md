# Work Manager swimlanes and a mentions inbox

Design, agreed with the owner on 2026-09-25. There are two independent parts. Either can ship without the other.

## Part 1: swimlanes by direct parent

### 1.1 Why

On the Work Manager board, a sprint reads as a flat list. Grouping cards by the work item they belong to makes each story's tasks sit together under it.

### 1.2 Owner decisions

1. Cards are grouped by their **direct parent**:
   - tasks and bugs under their PBI;
   - PBIs under their Feature.

   There is no walking up the tree. This matches Azure DevOps' own "group by parent".
2. It is a view option, **off by default**.

### 1.3 Data

- `work_board::BOARD_FIELDS` gains `System.Parent`, a plain integer field.
- `BoardItem` gains `parent: Option<BoardParent>`, where `BoardParent { id: i32, title: String, work_item_type: String }`.
- **Reading the parents.** `fetch_board`, after reading the cards, collects the distinct parent ids and batch-reads `System.Title,System.WorkItemType`. It reads in batches of 200, the same way it reads the cards, using `errorPolicy=omit` so a missing id does not fail the batch.
- **Unreadable parents.** A parent that cannot be read (deleted, in another project, or no permission) becomes `BoardParent { id, title: "", work_item_type: "" }`. Its lane reads `#1234`.
- **Failures.** The parent read never fails the board. If the whole read fails, it is logged, and every card keeps its parent id with an empty title.
- Bindings are regenerated.

### 1.4 The view

- **The switch.** A **Swimlanes** switch in the board toolbar. It is remembered on this machine in localStorage (`tcm-v2-board-swimlanes`) and starts off. With it off, the board is exactly as today.
- **Lanes.** With it on, each distinct parent gets a lane:
  - **Header row:** the parent's type label, `#id` and title (or just `#id` when unreadable), and the lane's card count.
  - **Below it:** the usual To Do / In Progress / Done columns, holding only that parent's cards.
  - **Opening the parent:** clicking the title opens it in the work-item drawer, as a card click does.
- **Order.** Lanes are ordered by their most recently changed card, which is the board's current card order. The **No parent** lane always comes last.
- **Filters.** Filters (type, assignee, text, sprint) apply before grouping. A lane with no cards after filtering is not shown.
- **Collapsing.**
  - Each lane collapses to its header row.
  - Collapsed lanes are remembered per organisation and project in localStorage (`tcm-v2-board-lanes-collapsed:<org>/<project>`), as a list of parent ids, with `0` for No parent.
  - **Collapse all** and **Expand all** sit beside the switch.
- **Collapsed columns.** Collapsed columns (`tcm-v2-hidden-cols`) collapse in every lane alike.
- **Dragging.** Dragging a card to another column changes its state, exactly as today. A card can only be dropped in its own lane's columns. Dropping never changes a parent.
- **A card that is itself a parent.** It appears in its own parent's lane. For example, a PBI card sits in its Feature's lane while its tasks sit in the PBI's lane.
- **Conventions.** Theme tokens, the shared `Switch`, icons from `src/lib/actionIcons.ts`, and no em dashes. `src/ui-consistency.test.ts` and `src/a11y.test.tsx` are not weakened. The lane header is a button with an accessible name: "Leave requests, 4 cards, collapse" or "expand".

### 1.5 Not in scope

- Grouping by Feature across levels.
- Changing a card's parent by dragging.
- Ordering lanes by backlog priority.

## Part 2: mentions in the notification bell

### 2.1 Why

When someone @mentions you in Azure DevOps, the bell should say so. It already reports new assignments and PR activity.

### 2.2 Owner decisions

1. Mentions come from **work-item discussions** and from comments on **PRs you are on**: the active PRs you created or review, which the app already polls. Other PRs are not scanned.
2. Mentions are a new notification kind with its own label, raised like the existing kinds.

### 2.3 Who "you" are

Your Azure DevOps identity id comes from `connectionData`, through the existing `connected_user` command. It is read once per organisation per session and cached in memory only.

### 2.4 Work-item mentions

- **The command.** A Rust command, `recent_mentions(organization, project) -> Result<Vec<Mention>, AdoError>`.
- **Finding the items.** It runs this WIQL for the project, then keeps the 20 most recently changed:

  `SELECT [System.Id] FROM WorkItems WHERE [System.Id] IN (@RecentMentions) ORDER BY [System.ChangedDate] DESC`

  `@RecentMentions` is Azure DevOps' macro for the items that mentioned you in the last 30 days.
- **Reading the comments.** For each item it reads the newest comments (`/_apis/wit/workItems/{id}/comments?order=desc&$top=50`) and keeps each comment where both of these hold:
  - its HTML contains a mention of your id, the anchor `data-vss-mention="version:2.0,{your id}"`, compared case-insensitively;
  - its author is not you.
- **The result.** `Mention { source: "work-item", item_id, item_type, item_title, comment_id, author, excerpt, created_date }`.
  - `excerpt` is the comment's text with the HTML stripped and whitespace collapsed, cut at 140 characters.
- **Failures.** One item's comment read failing skips that item. The WIQL failing fails the command, and the caller logs it and tries again at the next check.

### 2.5 PR mentions

- The app already reads the threads of each PR you are on (`prThreads` in `usePrAttention.ts`). The same comments are scanned for `@<{your id}>` (case-insensitive), skipping comments you wrote.
- No new request is made.
- Each match is a `Mention` with `source: "pr"`, carrying the repo, the PR id, thread and comment ids, author, excerpt and date.

### 2.6 Raising them

- **The notification kind.** `NotificationKind` gains `"mention"`, with label **Mention** and its own token colour in the bell.
- **Ids.**
  - Work-item mentions: `mention:wi:<item>:<comment>`.
  - PR mentions: `mention:pr:<repo>:<pr>:<thread>:<comment>`.

  `raise` already ignores an id it has seen, so each mention appears once and a dismissed one stays dismissed.
- **Text.**
  - **Title:** `<author> mentioned you on <Type> #<id>`, or `<author> mentioned you on PR #<id>`.
  - **Body:** the excerpt.
  - **Target:** the work item, or the PR, so that clicking opens it in the app through the existing routing.
  - **`href`:** Azure DevOps' own page.
- **When it checks.** Work-item mentions are checked every 5 minutes, and when the app starts, from a hook mounted beside `usePrAttention` (react-query, `refetchInterval` 5 minutes). PR mentions are checked whenever the PR threads refresh.
- **The first check.** On the first check ever for an organisation, only mentions from the last 24 hours are raised. Older ones are recorded as seen without showing, so the first run does not flood the bell. This is stored as a per-organisation marker in localStorage (`tcm-v2-mentions-baseline:<org>`).
- **Toast.** A newly raised mention also shows a toast, or an OS notification when the app is not in view, the same as a new assignment (`appIsInView` / `osNotify`).
- **Failures.** A failed check is logged (`applog`) and retried at the next check, with no toast.

### 2.7 Not in scope

- Scrolling to the exact comment.
- Mentions in a work item's description or other fields. Only discussion comments count.
- Mentions on PRs you are not on.
- Group mentions, such as `@team`.

## 3. Testing

**Rust (`src-tauri/tests/`):**
- The board read carries each card's parent. Parents are batch-read, and an unreadable one gets an empty title.
- `recent_mentions`:
  - finds a comment mentioning you;
  - ignores comments by you and comments that mention someone else;
  - strips HTML in the excerpt;
  - one item's failure skips that item only.

  These use the existing mocked ADO client patterns.

**Frontend:**
- **Swimlanes:**
  - off by default, with the board unchanged;
  - lanes grouped by parent, ordered, with No parent last;
  - filters hide empty lanes;
  - lanes collapse and the collapse is remembered;
  - Collapse all / Expand all;
  - a drop is limited to its own lane;
  - clicking a lane title opens the drawer.
- **Mentions:**
  - the notification ids and text;
  - the PR mention scan;
  - the first-run 24-hour baseline;
  - no self-mentions;
  - the Mention label in the bell;
  - clicking opens the target.

**By hand, needs a live organisation:** someone @mentions you on a work item and on a PR you are on, and both reach the bell within 5 minutes. Also check the swimlanes on your real sprint board.
