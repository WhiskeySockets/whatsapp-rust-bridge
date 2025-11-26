import { describe, it, expect } from "bun:test";
import { extractImageThumb, generateProfilePicture } from "../dist";

// 1x1 red PNG pixel (base64)
const SAMPLE_IMAGE =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAACXBIWXMAAAABAAAAAQBPJcTWAAAADElEQVR4nGP8x8AAAAMCAQBFsWYPAAAAAElFTkSuQmCC";

describe("Image Utils", () => {
  const imageBuffer = Buffer.from(SAMPLE_IMAGE, "base64");

  it("extracts an image thumbnail", () => {
    const result = extractImageThumb(imageBuffer, 32) as any;

    expect(result).toBeDefined();
    expect(result.original).toBeDefined();
    expect(result.original.width).toBe(1);
    expect(result.original.height).toBe(1);

    expect(result.buffer).toBeInstanceOf(Uint8Array);
    expect(result.buffer.length).toBeGreaterThan(0);
  });

  it("generates a square profile picture", () => {
    const targetWidth = 64;
    const result = generateProfilePicture(imageBuffer, targetWidth) as any;

    expect(result).toBeDefined();
    expect(result.img).toBeInstanceOf(Uint8Array);
    expect(result.img.length).toBeGreaterThan(0);
  });

  it("throws on invalid image data", () => {
    const invalidBuffer = new Uint8Array([0, 1, 2, 3]);
    expect(() => extractImageThumb(invalidBuffer, 32)).toThrow();
  });
});
