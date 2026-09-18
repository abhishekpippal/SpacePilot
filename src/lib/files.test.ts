import { describe, expect, it } from "vitest";
import { filterAndSortFiles } from "./files";
import type { FileEntry } from "../types";

const file = (name:string,size:number,modified:string|null):FileEntry => ({path:name,name,extension:"bin",size,modified,category:"Other",is_directory:false});

describe("large-file filtering", () => {
  const files=[file("z",200,"2024-01-01T00:00:00Z"),file("a",100,"2025-01-01T00:00:00Z")];
  it("filters using actual byte sizes",()=>expect(filterAndSortFiles(files,150,"size").map(item=>item.name)).toEqual(["z"]));
  it("sorts by name and modification time",()=>{
    expect(filterAndSortFiles(files,0,"name").map(item=>item.name)).toEqual(["a","z"]);
    expect(filterAndSortFiles(files,0,"modified").map(item=>item.name)).toEqual(["a","z"]);
  });
});
