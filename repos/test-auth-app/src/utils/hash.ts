import * as crypto from "crypto";

const SALT_LENGTH = 32;
const KEY_LENGTH = 64;
const SCRYPT_COST = 16384;

export function generateSalt(): string {
  return crypto.randomBytes(SALT_LENGTH).toString("hex");
}

export async function hashPassword(password: string): Promise<string> {
  const salt = generateSalt();
  return new Promise((resolve, reject) => {
    crypto.scrypt(password, salt, KEY_LENGTH, { N: SCRYPT_COST }, (err: Error | null, derivedKey: Buffer) => {
      if (err) {
        reject(err);
        return;
      }
      resolve(`${salt}:${derivedKey.toString("hex")}`);
    });
  });
}

export async function comparePassword(password: string, storedHash: string): Promise<boolean> {
  const [salt, hash] = storedHash.split(":");
  if (!salt || !hash) return false;

  return new Promise((resolve, reject) => {
    crypto.scrypt(password, salt, KEY_LENGTH, { N: SCRYPT_COST }, (err: Error | null, derivedKey: Buffer) => {
      if (err) {
        reject(err);
        return;
      }
      resolve(crypto.timingSafeEqual(Buffer.from(hash, "hex"), derivedKey));
    });
  });
}
