import { bench, do_not_optimize, boxplot, summary, run } from "mitata";
import { generateAudioWaveform } from "../dist/index.js";
import { getAudioWaveform as getAudioWaveformOld } from "baileys";
import fs from "node:fs";

const fileBuffer = fs.readFileSync("./assets/sonata.mp3");

boxplot(() => {
  summary(() => {
    bench("Waveform wasm/rust", () => {
      const waveform = generateAudioWaveform(fileBuffer);
      do_not_optimize(waveform);
    });

    bench("Waveform libsignal-node", async () => {
      const waveform = await getAudioWaveformOld(fileBuffer);
      do_not_optimize(waveform);
    });
  });
});

await run();
