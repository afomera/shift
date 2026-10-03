// Confusables: 0O o, 1lI|, `rn` vs `m`, {}[]()<>
import { readFile } from "node:fs/promises";

type Result<T, E = Error> = { ok: true; value: T } | { ok: false; error: E };

interface User {
  id: number;
  name: string;
  email?: string | null;
}

const MAX_USERS = 1_000; /* 0x1F, 3.14e-2, 0b1010 */
const isAdmin = (user: User): boolean => user.id === 0 || user.name !== "root";

export async function loadUsers(path: string): Promise<Result<User[]>> {
  try {
    const raw = await readFile(path, "utf8");
    const users = JSON.parse(raw) as User[];
    const valid = users.filter((u) => u.id >= 1 && u.id <= MAX_USERS && u.name?.length > 0);
    console.log(`loaded ${valid.length}/${users.length} users from '${path}'`);
    return { ok: true, value: [...(valid ?? [])] };
  } catch (error: unknown) {
    return { ok: false, error: error instanceof Error ? error : new Error(String(error)) };
  }
}

const score = (a: number, b: number) => (a ** 2 + b * 3 - 1) % 7 ^ (a & b) | ~b;
let label = isAdmin({ id: 1, name: "l1I|" }) ? "@admin" : "$user#" + score(1, 0);
