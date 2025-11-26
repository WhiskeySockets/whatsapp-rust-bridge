import { describe, it, expect } from "bun:test";
import { generateAudioWaveform } from "../dist";

const MP3_BASE64 =
  "SUQzBAAAAAAAIlRTU0UAAAAOAAADTGF2ZjYyLjMuMTAwAAAAAAAAAAAAAAD/+0DAAAAAAAAAAAAAAAAAAAAAAABJbmZvAAAA" +
  "DwAAAAUAAAK+AGhoaGhoaGhoaGhoaGhoaGhoaGiOjo6Ojo6Ojo6Ojo6Ojo6Ojo6OjrS0tLS0tLS0tLS0tLS0tLS0tLS02tra2tr" +
  "a2tra2tra2tra2tra2tr//////////////////////////wAAAABMYXZjNjIuMTEAAAAAAAAAAAAAAAAkAwYAAAAAAAACvhC6F/0AA" +
  "AAAAP/7EMQAA8AAAaQAAAAgAAA0gAAABExBTUUzLjEwMFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV" +
  "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//sQxCmDwAABpAAAACAAADSAAAAEVVVVVVVVVVVVVVVVVVVVVVVVVVVV" +
  "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVX/+xDEUwPAAAGkAAAAIAAANIAAAAR" +
  "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV" +
  "VVVVVVVVVVVf/7EMR8g8AAAaQAAAAgAAA0gAAABFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV" +
  "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//sQxKYDwAABpAAAACAAADSAAAAEVVVVVVVVVVVVVVVVVVVVVVVVV" +
  "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVU=%";

describe("Audio Waveform Generation", () => {
  it("creates a 64-sample waveform from MP3 audio", () => {
    const audioBuffer = Buffer.from(MP3_BASE64, "base64");
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
