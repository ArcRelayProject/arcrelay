// Browser preview only. Native desktop code uses the core TOTP implementation.
import type { Totp, TotpCode } from "../../ipc/generated";

export async function previewTotp(config: Totp, nowMs: number): Promise<TotpCode> {
  let { secret, algorithm, digits, period } = config;
  if (secret.length > 8192 || !Number.isSafeInteger(nowMs) || nowMs < 0)
    throw Error("无效的 TOTP 配置或系统时间");
  if (secret.startsWith("otpauth://")) {
    const uri = new URL(secret);
    if (uri.hostname !== "totp") throw Error("仅支持 TOTP");
    const seen = new Set<string>();
    for (const [key] of uri.searchParams) {
      if (seen.has(key)) throw Error("TOTP 参数重复");
      seen.add(key);
    }
    secret = uri.searchParams.get("secret") ?? "";
    algorithm = uri.searchParams.get("algorithm") ?? algorithm;
    digits = Number(uri.searchParams.get("digits") ?? digits);
    period = Number(uri.searchParams.get("period") ?? period);
  }
  secret = secret.replace(/\s/g, "").toUpperCase().replace(/=+$/, "");
  algorithm = algorithm.toUpperCase();
  if (
    !["SHA1", "SHA256", "SHA512"].includes(algorithm) ||
    ![6, 8].includes(digits) ||
    !Number.isInteger(period) ||
    period < 15 ||
    period > 120
  )
    throw Error("请检查 TOTP 算法、位数和周期");
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const bytes: number[] = [];
  let bits = 0,
    buffer = 0;
  for (const char of secret) {
    const value = alphabet.indexOf(char);
    if (value < 0) throw Error("TOTP 密钥必须是 Base32");
    buffer = (buffer << 5) | value;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >> bits) & 255);
      buffer &= (1 << bits) - 1;
    }
  }
  if (bits >= 5 || buffer !== 0 || bytes.length < 10 || bytes.length > 128)
    throw Error("TOTP 密钥无效");
  const step = period * 1000;
  const counter = new ArrayBuffer(8);
  new DataView(counter).setBigUint64(0, BigInt(Math.floor(nowMs / step)));
  const key = await crypto.subtle.importKey(
    "raw",
    new Uint8Array(bytes),
    { name: "HMAC", hash: algorithm.replace("SHA", "SHA-") },
    false,
    ["sign"],
  );
  const hash = new Uint8Array(await crypto.subtle.sign("HMAC", key, counter));
  const offset = hash[hash.length - 1] & 15;
  const value = new DataView(hash.buffer).getUint32(offset) & 0x7fffffff;
  return {
    code: String(value % 10 ** digits).padStart(digits, "0"),
    expiresAtMs: (Math.floor(nowMs / step) + 1) * step,
  };
}
