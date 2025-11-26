import { bench, do_not_optimize, boxplot, summary, run } from "mitata";
import { generateProfilePicture, extractImageThumb } from "../dist/index.js";
import {
  generateProfilePicture as generateProfilePictureOld,
  extractImageThumb as extractImageThumbOld,
} from "baileys";
import fs from "node:fs";

const fileBuffer = fs.readFileSync("./assets/image.png");

boxplot(() => {
  summary(() => {
    bench("Profile Picture wasm/rust", async () => {
      const profilePicture = generateProfilePicture(fileBuffer, 96);
      do_not_optimize(profilePicture);
    });

    bench("Profile Picture libsignal-node", async () => {
      const profilePicture = await generateProfilePictureOld(fileBuffer, {
        width: 96,
        height: 96,
      });
      do_not_optimize(profilePicture);
    });
  });

  summary(() => {
    bench("Extract thumbnail wasm/rust", async () => {
      const profilePicture = extractImageThumb(fileBuffer, 96);
      do_not_optimize(profilePicture);
    });

    bench("Extract thumbnail libsignal-node", async () => {
      const profilePicture = await extractImageThumbOld(fileBuffer, 96);
      do_not_optimize(profilePicture);
    });
  });
});

await run();
