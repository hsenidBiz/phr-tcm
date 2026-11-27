/**
 * How the report page (src-tauri/web/autorun-report.js) hands videos to the
 * player: a video opens it with the case's timeline, pictures keep the
 * lightbox (over the pictures only), and while the player is open the page's
 * own keys stand down.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent } from "@testing-library/dom";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(resolve(here, "../../src-tauri/web/autorun-report.js"), "utf8");

const timeline = { title: "Video of #2 B", length_ms: 9000, steps: [], marks: [] };
const data = {
  gallery: [
    { n: "shot-1-1.jpg", c: 1, s: 1 },
    { n: "video-2-1.mp4", c: 2, k: 1 },
    { n: "shot-3-1.jpg", c: 3, s: 3 },
  ],
  videos: { "video-2-1.mp4": timeline },
};

let playerOpen = false;
const open = vi.fn();

function mount(d: unknown) {
  playerOpen = false;
  open.mockClear();
  (window as unknown as { ReportPlayer: unknown }).ReportPlayer = { open, isOpen: () => playerOpen, close: vi.fn() };
  document.body.innerHTML = `
    <script id="report-data" type="application/json">${JSON.stringify(d)}</script>
    <div class="app"><div id="view-gallery" class="view">
      <button class="tab" data-gtab="all" aria-selected="true">All</button>
      <ul id="gal-grid"></ul><button id="gal-more" hidden>More</button>
    </div></div>
    <a href="#" data-video="video-2-1.mp4" id="vlink">Video</a>
    <div id="lb" hidden><div class="stage"></div><span class="lb-cap"></span>
      <button class="lb-x">x</button><button class="lb-prev">p</button><button class="lb-next">n</button></div>`;
  new Function(src)();
  fireEvent.click(document.querySelector(".tab[data-gtab]")!);
}

beforeEach(() => mount(data));

const tiles = () => Array.from(document.querySelectorAll<HTMLElement>(".gallery__media"));
const lb = () => document.getElementById("lb") as HTMLElement;
const caption = () => lb().querySelector(".lb-cap")!.textContent;

test("a video link opens the player with that video's timeline and the link as opener", () => {
  const link = document.getElementById("vlink")!;
  fireEvent.click(link);
  expect(open).toHaveBeenCalledWith(timeline, "../shots/video-2-1.mp4", link);
  expect(lb().hidden).toBe(true);
});

test("a video tile opens the player; a video without a timeline gets an empty one", () => {
  fireEvent.click(tiles()[1]);
  expect(open).toHaveBeenCalledWith(timeline, "../shots/video-2-1.mp4", tiles()[1]);
  mount({ gallery: data.gallery });
  fireEvent.click(document.getElementById("vlink")!);
  expect(open.mock.calls[open.mock.calls.length - 1]![0]).toEqual({ title: "Video", length_ms: null, steps: [], marks: [] });
});

test("a picture opens the lightbox over the pictures only", () => {
  fireEvent.click(tiles()[2]);
  expect(open).not.toHaveBeenCalled();
  expect(caption()).toBe("#3 3  (2 of 2)");
  fireEvent.click(lb().querySelector(".lb-next")!);
  expect(caption()).toBe("#1 1  (1 of 2)");
});

test("while the player is open, Escape and the arrows leave the lightbox alone", () => {
  fireEvent.click(tiles()[0]);
  expect(lb().hidden).toBe(false);
  playerOpen = true;
  fireEvent.keyDown(document, { key: "ArrowRight" });
  expect(caption()).toBe("#1 1  (1 of 2)");
  fireEvent.keyDown(document, { key: "Escape" });
  expect(lb().hidden).toBe(false);
  playerOpen = false;
  fireEvent.keyDown(document, { key: "ArrowRight" });
  expect(caption()).toBe("#3 3  (2 of 2)");
  fireEvent.keyDown(document, { key: "Escape" });
  expect(lb().hidden).toBe(true);
});
