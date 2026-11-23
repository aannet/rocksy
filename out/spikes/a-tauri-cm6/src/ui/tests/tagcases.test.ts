// The tag filter the UI applies to its cached list must give what the core's `tags_list` gives.
// The cases are shared with the core: testdata/tags_list_cases.json is also played by
// crates/rocksy-core/tests/tags_list_cases.rs against the real index.
import raw from "../../testdata/tags_list_cases.json?raw";
import { describe, expect, it } from "vitest";
import { filterTagLibrary, makeTagLibrary } from "../src/tagtree";
import type { TagCount } from "../src/types";

type Cases = { notes: string[]; library: TagCount[]; cases: { query: string; expected: string[] }[] };
const shared: Cases = JSON.parse(raw);

describe("tags_list: cases shared with the core", () => {
  const lib = makeTagLibrary(shared.library);

  it("has the cases the core test plays", () => {
    expect(shared.library.length).toBeGreaterThan(15);
    expect(shared.cases.length).toBeGreaterThanOrEqual(15);
  });

  for (const c of shared.cases) {
    it(`query ${JSON.stringify(c.query)}`, () => {
      const got = filterTagLibrary(lib, c.query);
      expect(got.map((t) => t.tag)).toEqual(c.expected);
      // Entries are those of the library (same counts), not copies with other values.
      for (const t of got) expect(shared.library).toContainEqual(t);
    });
  }

  it("the empty query gives the library in its own order", () => {
    expect(filterTagLibrary(lib, "")).toEqual(shared.library);
  });
});
