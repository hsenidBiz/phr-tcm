// The review page: the browser page that View in browser (Import Test
// Cases) and View All Test Cases in Browser (View Test Cases) open, and
// the Test map it links to.
//
// These are pages the app writes for the browser, not app screens: every
// shot starts with a `reviewPage` step, which has the capture app write the
// page from src/dev/reviewSample.ts and opens it in Edge. Locates are CSS
// selectors into that page's own markup (src-tauri/src/import_parser/html.rs,
// src-tauri/web/cases-page.js, src-tauri/src/test_map.rs). The sample's
// first case is a new one (data-key "d0") carrying one of everything a case
// shows, so no shot scrolls - a scrolled page shifted under the high-density
// capture; its second is an update of #4312.

import type { Screen, Step } from "../types";

const PAGE = "review-page";
const OPTIONS = "review-page-options";
const MAP = "review-page-map";

const OPEN: Step = { reviewPage: "cases" };
const FIRST = '[data-key="d0"]';
const UPDATE = '[data-key="4312"]';

export const reviewPage: Screen = {
  id: "review-page",
  title: "Review page",
  group: "Test cases",
  summary:
    "The test cases as one page in your browser, for reading them through and leaving comments. **View in browser** on Import Test Cases opens it for a draft, " +
    "and **View All Test Cases in Browser** on View Test Cases for the cases already in Azure DevOps. It opens in the app's theme and stays in step with the app while it is open.",
  shots: [
    {
      id: PAGE,
      route: [OPEN, { click: { css: `${FIRST} .tc-mark` } }, { waitFor: { css: "#tc-goto" } }],
      alt: "The review page: the search bar, the first test case with its notes, finding, steps and comment, and the general comments and spec beside them",
    },
    {
      id: OPTIONS,
      route: [OPEN, { click: { css: ".tc-menu > summary" } }, { waitFor: { css: "#tc-notes" } }],
      alt: "The review page's Options menu",
    },
    {
      id: MAP,
      route: [
        { reviewPage: "map" },
        { activate: { css: '#map-list button.case[data-i="0"]' } },
        { waitFor: { css: "#detail h2" } },
      ],
      alt: "The Test map: the test cases as a tree of their areas, with one case opened beside it",
    },
  ],
  groups: [
    { id: "search", title: "Search and options", summary: "Find a case, and choose what the page shows." },
    { id: "case", title: "A test case", summary: "Everything about one case, and your comment on it." },
    { id: "side", title: "The whole set and the spec", summary: "Comments about a file as a whole, the specification beside the cases, and light or dark." },
    { id: "map", title: "The Test map", summary: "The same cases as a tree of their areas." },
  ],
  controls: [
    // --- Search and options ----------------------------------------------------------
    {
      id: "scheme",
      shot: PAGE,
      // In the corner above the side column: in "search" it stretched that
      // group's outline across the whole header.
      group: "side",
      locate: { css: "#scheme-switch" },
      name: "Light / Dark",
      does: "Switches the page between light and dark. It starts in the app's own theme.",
    },
    {
      id: "search-in",
      shot: PAGE,
      group: "search",
      locate: { css: ".tc-pick-btn" },
      name: "Search in",
      does: "Where the search looks: **All fields**, or just the title, ID, prerequisites, steps, tags or module.",
    },
    {
      id: "search-box",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-search" },
      name: "Search",
      does:
        "Shows only the cases that match, with what it found highlighted. Every word must appear somewhere; put words in quotes to find them together, as written. [[Esc]] clears it.",
    },
    {
      id: "match-case",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-case" },
      name: "Match case",
      does: "[[Aa]]: capitals count, so Login no longer finds login. [[Alt+C]] while typing.",
    },
    {
      id: "whole-word",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-word" },
      name: "Match whole word",
      does: "[[ab]]: only whole words, so log no longer finds login. [[Alt+W]] while typing.",
    },
    {
      id: "regex",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-regex" },
      name: "Regular expression",
      does: "[[.*]]: searches with a pattern, such as pay(ment|out). A pattern that is not finished outlines the box and says so. [[Alt+R]] while typing.",
    },
    {
      id: "goto",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-goto" },
      name: "Go to bookmark",
      does: "Scrolls back to the case you bookmarked. It appears once a case is bookmarked.",
    },
    {
      id: "options",
      shot: PAGE,
      group: "search",
      locate: { css: ".tc-menu > summary" },
      name: "Options",
      does: "Opens the menu of what the page shows (below).",
    },
    {
      id: "count",
      shot: PAGE,
      group: "search",
      locate: { css: "#tc-count" },
      name: "Count",
      does: "How many cases there are, or while searching, how many of them are shown.",
    },
    {
      id: "hide-notes",
      shot: OPTIONS,
      group: "search",
      locate: { css: "#tc-notes" },
      name: "Hide reviewer notes",
      does: "Folds away every case's reviewer notes, and **Show reviewer notes** brings them all back.",
    },
    {
      id: "hide-findings",
      shot: OPTIONS,
      group: "search",
      locate: { css: "#tc-findings" },
      name: "Hide findings",
      does: "Folds away the findings on every case, and **Show findings** brings them back.",
    },
    {
      id: "view-as-tree",
      shot: OPTIONS,
      group: "search",
      locate: { css: "#tc-tree" },
      name: "View as Tree",
      does: "Opens the Test map (below). It is offered when the cases have areas.",
    },
    {
      id: "hide-spec",
      shot: OPTIONS,
      group: "search",
      locate: { css: "#tc-spec" },
      name: "Hide spec",
      does: "Hides the specification beside the cases, for more room to read them.",
    },
    {
      id: "show-on-cards",
      shot: OPTIONS,
      group: "search",
      locate: { css: ".tc-menu-group" },
      name: "Show on cards",
      does: "Untick Automation Status, Module or Tags to hide them from every case. Only the ones your cases have are listed.",
    },

    // --- A test case -----------------------------------------------------------------
    {
      id: "bookmark",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} .tc-mark` },
      name: "Bookmark",
      does: "Marks where you stopped reading. There is one bookmark; marking another case moves it, and pressing it again clears it.",
    },
    {
      id: "new",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} .op-new` },
      name: "NEW",
      does: "On a draft: uploading will create this case.",
    },
    {
      id: "update",
      shot: PAGE,
      group: "case",
      locate: { css: `${UPDATE} .op-update` },
      name: "UPDATE",
      does: "On a draft: uploading will update the work item it names.",
    },
    {
      id: "fields",
      shot: PAGE,
      group: "case",
      // The card's own row - a finding carries a .meta line of its own.
      locate: { css: `${FIRST} > .meta` },
      name: "Automation Status, Module, Tags",
      does: "The case's fields, each on a row of its own. **Show on cards** in Options hides any of them.",
    },
    {
      id: "prerequisites",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} .pre` },
      name: "Prerequisites",
      does: "What has to be true before the first step. Every case shows it, as None when there is nothing.",
    },
    {
      id: "reviewer-notes",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} .rev > summary` },
      name: "Reviewer notes",
      does: "Where the case came from in the specification, often quoting it. The **×** folds away just this case's notes.",
    },
    {
      id: "steps",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} table` },
      name: "Steps",
      does: "Each action and the result expected from it, in order.",
    },
    {
      id: "comment",
      shot: PAGE,
      group: "case",
      locate: { css: "#nb-d0" },
      name: "My comment",
      does:
        "Your comment on the case. It saves by itself as you type, and says when it has. On a draft it is saved into the case's file; on cases already in Azure DevOps it is kept in the app, on this computer only.",
    },
    {
      id: "findings",
      shot: PAGE,
      group: "case",
      locate: { css: `${FIRST} .findings > summary` },
      name: "Findings",
      does: "Problems the AI assistant found while writing the case: in the case itself, the specification or the code. The **×** folds them away.",
    },

    // --- The whole set and the spec ------------------------------------------------------
    {
      id: "general-comments",
      shot: PAGE,
      group: "side",
      locate: { css: ".aside > summary" },
      name: "General comments",
      does: "On a draft imported from files: a comment box for each file, about the set as a whole. Press the heading to fold the column away.",
    },
    {
      id: "file-comment",
      shot: PAGE,
      group: "side",
      locate: { css: "#nb-f0" },
      name: "Comment on a file",
      does: "Saved into that file, like the comments on its cases.",
    },
    {
      id: "spec-tabs",
      shot: PAGE,
      group: "side",
      locate: { css: ".spec-tabs" },
      name: "Specification",
      does:
        "The specification the file names, beside the cases: a tab for each document, a file or an Azure DevOps wiki page. A **Spec:** line in a case's reviewer notes links to its place here.",
    },
    {
      id: "spec-grip",
      shot: PAGE,
      group: "side",
      locate: { css: ".spec-grip" },
      name: "Resize",
      does: "Drag to make the specification wider or narrower; double-click to put it back.",
    },

    // --- The Test map -------------------------------------------------------------------
    {
      id: "map-back",
      shot: MAP,
      group: "map",
      locate: { css: "#map-back" },
      name: "← Test cases",
      does: "Back to the review page.",
    },
    {
      id: "map-expand",
      shot: MAP,
      group: "map",
      locate: { css: "#map-expand" },
      name: "Expand all",
      does: "Opens every area. Click an area on the map to fold or open just that one.",
    },
    {
      id: "map-collapse",
      shot: MAP,
      group: "map",
      locate: { css: "#map-collapse" },
      name: "Collapse all",
      does: "Folds every area to its name.",
    },
    {
      id: "map-zoom",
      shot: MAP,
      group: "map",
      locate: { css: "#map-in" },
      name: "Zoom",
      does: "**-** and **+** zoom out and in, and **Reset** fits the whole map again. You can also drag the map around.",
    },
    {
      id: "map-graph",
      shot: MAP,
      group: "map",
      locate: { css: "#graph" },
      name: "The map",
      does: "Each area of the project, and the cases in it. Click a case to read it beside the map.",
    },
    {
      id: "map-detail",
      shot: MAP,
      group: "map",
      locate: { css: "#detail h2" },
      name: "The case",
      does: "The case you clicked: New or Update, its tags and status, prerequisites and steps. **Close** puts the map back to full width.",
    },
  ],
  tips: [
    "Once you have left comments on a draft, you can hand them to your AI assistant: tell it **Address the comments I made for the test cases**. It reads each comment from the case file, fixes the cases, and the review page offers to refresh when the file changes.",
    "The page is a file on this computer, so it works without a connection. Keep it open while you work: when the cases change in the app, a bar offers to refresh it.",
  ],
  howTo: [
    {
      title: "Have your AI assistant address your comments",
      steps: [
        "On Import Test Cases, press **View in browser**.",
        "Type your comments in the **My comment** box under each case, and in **General comments** for the set as a whole. They save into the case file by themselves.",
        "In your AI assistant, ask: **Address the comments I made for the test cases**.",
        "It reads every comment, fixes the cases in the file, and tells you what it changed. The review page then offers to refresh, so you can check the result.",
      ],
    },
  ],
};
