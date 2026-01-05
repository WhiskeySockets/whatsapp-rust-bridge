import { describe, it, expect } from "bun:test";
import { generateAudioWaveform, getAudioDuration } from "../dist";
import fs from "node:fs";

const EXPECTED_DURATION_SECONDS = 42.736326530612246;

const audioBuffer = fs.readFileSync("./assets/sonata.mp3");

describe("Audio Waveform Generation", () => {
  it("creates a 64-sample waveform from MP3 audio", () => {
    const waveform = generateAudioWaveform(audioBuffer);

    expect(waveform).toBeInstanceOf(Uint8Array);
    expect(waveform.length).toBe(64);
    expect(Math.max(...waveform)).toBeLessThanOrEqual(100);
  });

  it("throws on invalid audio data", () => {
    const randomBytes = new Uint8Array(256).fill(0x55);
    expect(() => generateAudioWaveform(randomBytes)).toThrow();
  });

  it("throws on empty input", () => {
    expect(() => generateAudioWaveform(new Uint8Array())).toThrow();
  });
});

describe("Audio Duration", () => {
  it("returns duration for Uint8Array input", async () => {
    const duration = await getAudioDuration(audioBuffer);

    expect(duration).toBeGreaterThan(0);
    expect(duration).toBeGreaterThan(40);
    expect(duration).toBeLessThan(45);
    expect(duration).toBeCloseTo(EXPECTED_DURATION_SECONDS, 3);
  });

  it("supports ReadableStream input", async () => {
    const chunkSize = 64 * 1024;
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (let offset = 0; offset < audioBuffer.length; offset += chunkSize) {
          controller.enqueue(audioBuffer.subarray(offset, offset + chunkSize));
        }
        controller.close();
      },
    });

    const duration = await getAudioDuration(stream);
    expect(duration).toBeGreaterThan(40);
    expect(duration).toBeLessThan(45);
    expect(duration).toBeCloseTo(EXPECTED_DURATION_SECONDS, 3);
  });

  it("throws on invalid audio data", async () => {
    const randomBytes = new Uint8Array(256).fill(0x55);
    await expect(getAudioDuration(randomBytes)).rejects.toThrow();
  });

  it("throws on empty input", async () => {
    await expect(getAudioDuration(new Uint8Array())).rejects.toThrow();
  });
});
