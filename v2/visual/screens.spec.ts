/**
 * Visual regression: every main screen, screenshotted in demo mode and
 * diffed against the goldens in __screenshots__. This is the layer the
 * unit tests cannot cover - "does the app still LOOK right": overlap,
 * clipping, dead space, a theme regression.
 *
 * Demo mode makes it deterministic: fake org, fixed dataset, no network.
 * All screens run on OLED (the daily-driver theme) plus a Light pass of
 * two representative screens, so both palette directions stay guarded
 * without doubling the golden set.
 *
 * On failure: `npx playwright show-report` shows expected/actual/diff.
 * If the change was intentional, `npm run visual:update` + commit.
 */
import { expect, test, type Page } from "@playwright/test";

/** Boot straight into demo mode: flag on, demo prefs, tour done, and the
 * changelog marked seen so no modal covers the screen. */
async function bootDemo(page: Page, themeId: string) {
  await page.addInitScript(
    ([theme]) => {
      localStorage.setItem("tcm-v2-dev-demo", "on");
      localStorage.setItem("tcm-v2-theme-id", theme);
      localStorage.setItem("tcm-v2-tour-done", "yes");
      localStorage.setItem("tcm-v2-changelog-seen", "99.0.0");
      localStorage.setItem(
        "tcm-v2-prefs",
        JSON.stringify({
          org: "DemoOrg",
          project: "Demo Project",
          section: "manual",
          pbi: { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" },
          workMode: false,
        }),
      );
    },
    [themeId],
  );
  await page.goto("/");
  // Demo boot is ready once the sidebar exists.
  await page.getByRole("button", { name: "Manual Entry" }).waitFor();
}

/** Wait out entrance animations (GSAP is JS-driven - the config's
 * animations:"disabled" cannot freeze it) before shooting. */
async function settle(page: Page) {
  await page.waitForTimeout(900);
}

async function shoot(page: Page, name: string) {
  await settle(page);
  await expect(page).toHaveScreenshot(`${name}.png`, {
    fullPage: false,
    // Panels whose content moves with every release (changelog entries,
    // app log lines) are masked - otherwise shipping anything at all
    // reddens a golden that has nothing to do with the change.
    mask: [page.locator("[data-visual-mask]")],
  });
}

const CASE_TABS = [
  ["Manual Entry", "manual"],
  ["Import File", "import"],
  ["Update Test Cases", "edit"],
  ["View Test Cases", "view"],
  ["Run Tests", "run"],
  ["Test Suites", "suites"],
  ["AI Bridge", "ai"],
] as const;

test.describe("OLED (daily driver)", () => {
  test("test case manager screens", async ({ page }) => {
    await bootDemo(page, "oled");
    for (const [label, slug] of CASE_TABS) {
      await page.getByRole("button", { name: label, exact: true }).click();
      await shoot(page, `oled-${slug}`);
    }
  });

  test("settings", async ({ page }) => {
    await bootDemo(page, "oled");
    await page.getByRole("button", { name: "Settings" }).click();
    await shoot(page, "oled-settings");
  });

  test("work manager screens", async ({ page }) => {
    await bootDemo(page, "oled");
    await page.getByRole("button", { name: "Work Manager (Beta)" }).click();
    await page.getByRole("button", { name: "Board", exact: true }).waitFor();
    await shoot(page, "oled-work-board");
    await page.getByRole("button", { name: "Pull Requests", exact: true }).click();
    await shoot(page, "oled-work-prs");
    await page.getByRole("button", { name: "New Work Item", exact: true }).click();
    await shoot(page, "oled-work-create");
  });
});

test.describe("Light (palette inverse)", () => {
  // Two representative screens keep the light palette guarded without
  // doubling the golden set: the densest form and the densest list.
  test("manual entry + board", async ({ page }) => {
    await bootDemo(page, "light");
    await shoot(page, "light-manual");
    await page.getByRole("button", { name: "Work Manager (Beta)" }).click();
    await page.getByRole("button", { name: "Board", exact: true }).waitFor();
    await shoot(page, "light-work-board");
  });
});
