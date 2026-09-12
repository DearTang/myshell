/**
 * One-off: replicate local commits df0a707..3a0a9cb on GitHub via the Git
 * Data API, because the git transport (https proxy 503 / direct reset) is
 * down while api.github.com is reachable.
 *
 * SAFETY: every intermediate object (blob → tree → commit) must come back
 * with a SHA IDENTICAL to the local one, otherwise we abort WITHOUT
 * touching any ref — a mismatch would mean a divergent history. Only after
 * both commits verify do we fast-forward refs/heads/main and move the
 * v2.13.0 release tag to the right commit.
 */
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

const TOKEN = readFileSync(".github-token", "utf8").trim();
const REPO = "DearTang/myshell";
const API = `https://api.github.com/repos/${REPO}`;
const H = {
  Authorization: `Bearer ${TOKEN}`,
  "User-Agent": "myshell-release-repair",
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
};

async function api(path, body, method = "POST") {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: { ...H, "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(`${method} ${path} → ${res.status}: ${JSON.stringify(json).slice(0, 400)}`);
  }
  return json;
}

/** Parse a raw `git cat-file commit` object into its exact fields. */
function parseCommit(sha) {
  const raw = execSync(`git cat-file commit ${sha}`, { maxBuffer: 1 << 20 }).toString("utf8");
  const sep = raw.indexOf("\n\n");
  const header = raw.slice(0, sep);
  const message = raw.slice(sep + 2); // keep verbatim incl. trailing newline
  const out = { sha, message, parents: [] };
  for (const line of header.split("\n")) {
    if (line.startsWith("tree ")) out.tree = line.slice(5);
    else if (line.startsWith("parent ")) out.parents.push(line.slice(7));
    else if (line.startsWith("author ") || line.startsWith("committer ")) {
      const m = line.match(/^(author|committer) (.*) <(.*)> (\d+) ([+-]\d{4})$/);
      if (!m) throw new Error(`unparsable ${line}`);
      // Format the timestamp in the commit's own zone → ISO 8601 with offset.
      const epoch = Number(m[4]);
      const offMin = (Number(m[5].slice(1, 3)) * 60 + Number(m[5].slice(3))) * (m[5][0] === "-" ? -1 : 1);
      const d = new Date((epoch + offMin * 60) * 1000);
      const iso =
        `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}` +
        `-${String(d.getUTCDate()).padStart(2, "0")}T${String(d.getUTCHours()).padStart(2, "0")}` +
        `:${String(d.getUTCMinutes()).padStart(2, "0")}:${String(d.getUTCSeconds()).padStart(2, "0")}` +
        `${m[5].slice(0, 3)}:${m[5].slice(3)}`;
      out[m[1]] = { name: m[2], email: m[3], date: iso };
    }
  }
  return out;
}

/** Changed entries (mode, blob sha, path) of a commit vs its first parent. */
function diffEntries(sha) {
  const out = execSync(`git diff-tree -r --no-commit-id ${sha}`).toString("utf8").trim();
  return out
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      const m = l.match(/^:(\d+) (\d+) ([0-9a-f]+) ([0-9a-f]+) [A-Z]\t(.+)$/);
      if (!m) throw new Error(`unparsable diff line: ${l}`);
      return { mode: m[2], sha: m[4], path: m[5] };
    });
}

async function main() {
  const commits = ["0ac8e6ca3890bc3793116f6c5161b8adba3125e2", "3a0a9cb671092672dce032c32eed834d8681974d"];
  let baseTree = null; // resolved from the remote parent below

  for (const sha of commits) {
    const c = parseCommit(sha);
    const entries = diffEntries(sha);
    console.log(`— commit ${sha.slice(0, 8)}: ${entries.length} files, msg ${JSON.stringify(c.message.split("\n")[0])}`);

    // 1. Upload blobs (skip ones the API already has — matched by sha).
    for (const e of entries) {
      const content = execSync(`git cat-file blob ${e.sha}`, { maxBuffer: 1 << 27 }).toString("base64");
      const blob = await api("/git/blobs", { content, encoding: "base64" });
      if (blob.sha !== e.sha) {
        throw new Error(`blob mismatch for ${e.path}: local ${e.sha} vs remote ${blob.sha} — ABORT (no refs touched)`);
      }
    }
    console.log(`  blobs ok (${entries.length}/${entries.length})`);

    // 2. Build the tree on top of the previous one (first commit: remote main's tree).
    if (baseTree === null) {
      const parent = c.parents[0];
      const ref = await api(`/git/commits/${parent}`, undefined, "GET");
      baseTree = ref.tree.sha;
      console.log(`  base tree (from remote parent ${parent.slice(0, 8)}): ${baseTree}`);
    }
    const tree = await api("/git/trees", {
      base_tree: baseTree,
      tree: entries.map((e) => ({ path: e.path, mode: e.mode, type: "blob", sha: e.sha })),
    });
    if (tree.sha !== c.tree) {
      throw new Error(`tree mismatch: local ${c.tree} vs remote ${tree.sha} — ABORT (no refs touched)`);
    }
    console.log(`  tree ok: ${tree.sha.slice(0, 8)}`);
    baseTree = c.tree;

    // 3. Create the commit with identical metadata.
    const made = await api("/git/commits", {
      message: c.message,
      tree: c.tree,
      parents: c.parents,
      author: c.author,
      committer: c.committer,
    });
    if (made.sha !== sha) {
      throw new Error(`commit mismatch: local ${sha} vs remote ${made.sha} — ABORT (no refs touched)`);
    }
    console.log(`  commit ok: ${made.sha.slice(0, 8)} (SHA-identical)`);
  }

  // 4. Everything verified — fast-forward main and re-point the release tag.
  const target = commits[commits.length - 1];
  const upd = await api("/git/refs/heads/main", { sha: target, force: false }, "PATCH");
  console.log(`main → ${upd.object.sha.slice(0, 8)} (fast-forwarded)`);
  const tag = await api("/git/refs/tags/v2.13.0", { sha: target, force: true }, "PATCH");
  console.log(`tag v2.13.0 → ${tag.object.sha.slice(0, 8)} (release body/assets untouched)`);
  console.log("DONE — remote history is byte-identical to local.");
}

main().catch((e) => {
  console.error("FAILED:", e.message);
  process.exit(1);
});
