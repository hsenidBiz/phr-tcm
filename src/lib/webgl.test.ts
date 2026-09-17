import { describe, expect, test } from "vitest";
import { hasWebGL, isSoftwareRenderer } from "./webgl";

/// WebView2 can answer "yes, WebGL" while drawing it on the CPU: after an
/// update it put WebGL on WARP (Microsoft's software rasteriser), and the
/// sign-in screen's per-pixel shader then pinned every core. The gate has
/// to read the renderer's name, not just whether a context exists.
describe("isSoftwareRenderer", () => {
  test("names the software rasterisers WebView2 and Chromium fall back to", () => {
    expect(isSoftwareRenderer("ANGLE (Microsoft, Microsoft Basic Render Driver Direct3D11 vs_5_0 ps_5_0, D3D11-10.0.26100.1)")).toBe(true);
    expect(isSoftwareRenderer("ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)")).toBe(true);
    expect(isSoftwareRenderer("WARP")).toBe(true);
    expect(isSoftwareRenderer("Mesa llvmpipe (LLVM 15.0.7, 256 bits)")).toBe(true);
    expect(isSoftwareRenderer("Software Rasterizer")).toBe(true);
  });

  test("leaves real GPUs alone", () => {
    expect(isSoftwareRenderer("ANGLE (NVIDIA, NVIDIA GeForce RTX 5080 (0x00002C02) Direct3D11 vs_5_0 ps_5_0, D3D11-32.0.16.1074)")).toBe(false);
    expect(isSoftwareRenderer("ANGLE (AMD, AMD Radeon(TM) Graphics Direct3D11 vs_5_0 ps_5_0, D3D11-32.0.21045.5002)")).toBe(false);
    expect(isSoftwareRenderer("Apple M2")).toBe(false);
    expect(isSoftwareRenderer("")).toBe(false);
  });
});

test("hasWebGL is false where no context can be made (jsdom)", () => {
  expect(hasWebGL()).toBe(false);
});
