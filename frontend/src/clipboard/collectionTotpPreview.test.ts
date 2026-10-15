import assert from "node:assert/strict";
import test from "node:test";
import { previewTotp } from "../features/collections/previewTotp.ts";

const config = {
  secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
  algorithm: "SHA1",
  digits: 8,
  period: 30,
};
test("browser TOTP follows RFC 6238 across time steps", async () => {
  assert.deepEqual(await previewTotp(config, 59000), { code: "94287082", expiresAtMs: 60000 });
  assert.deepEqual(await previewTotp(config, 60000), { code: "37359152", expiresAtMs: 90000 });
  assert.equal((await previewTotp(config, 1111111109000)).code, "07081804");
});
test("browser TOTP supports otpauth parameters and rejects invalid keys", async () => {
  assert.equal(
    (
      await previewTotp(
        { ...config, digits: 6, secret: `otpauth://totp/Demo?secret=${config.secret}&digits=8` },
        59000,
      )
    ).code,
    "94287082",
  );
  for (const secret of [
    "invalid!",
    `otpauth://hotp/Demo?secret=${config.secret}`,
    `otpauth://totp/Demo?secret=${config.secret}&digits=8&digits=6`,
  ])
    await assert.rejects(previewTotp({ ...config, secret }, 59000));
});
