import { describe,expect,it } from "vitest";
import { filterRescueItems,normalizeRescueTargetGb,rescueDestination,rescueProgress } from "./rescue";
import type { FileEntry } from "../types";

const file=(path:string):FileEntry=>({path,name:path,extension:"bin",size:1,modified:null,category:"Other",is_directory:false});

describe("Space Rescue interactions",()=>{
  it("bounds invalid and extreme custom targets",()=>{
    expect(normalizeRescueTargetGb(Number.NaN)).toBe(1);
    expect(normalizeRescueTargetGb(-10)).toBe(1);
    expect(normalizeRescueTargetGb(200000)).toBe(100000);
  });
  it("routes review actions into existing pages",()=>{
    expect(rescueDestination("review_duplicates")).toBe("Duplicates");
    expect(rescueDestination("review_cleanup")).toBe("Cleanup");
    expect(rescueDestination("review_large_files")).toBe("Large Files");
  });
  it("counts only successful recovered bytes and clamps progress",()=>{
    expect(rescueProgress(100,30)).toEqual({percent:30,remaining:70});
    expect(rescueProgress(100,120)).toEqual({percent:100,remaining:0});
  });
  it("filters existing page data to plan paths without selecting it",()=>{
    const a=file("a"),b=file("b"),paths=new Set(["a"]);
    const result=filterRescueItems([a,b],[{hash:"h",size:1,files:[a,b],recoverable_size:1}],[],paths);
    expect(result.files).toEqual([a]);
    expect(result.groups).toHaveLength(1);
    expect(paths.has("b")).toBe(false);
  });
});
