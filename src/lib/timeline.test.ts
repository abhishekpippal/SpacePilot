import { describe,expect,it } from "vitest";
import { clampTimelineRange,historyPoints,timelineDestination,timelineState } from "./timeline";
import type { TimelineResponse } from "../types";

const base:TimelineResponse={snapshots:[],report:null,first_snapshot:false,insufficient_range:false,scope_root:null,warnings:[],history_bytes:0};

describe("Timeline interactions",()=>{
  it("distinguishes empty, first-snapshot, and historical states",()=>{
    expect(timelineState(null)).toBe("EMPTY");
    expect(timelineState({...base,first_snapshot:true,snapshots:[{timestamp:"2026-01-01",used_bytes:1,free_bytes:1,scanned_bytes:1}]})).toBe("FIRST_SNAPSHOT");
    expect(timelineState({...base,insufficient_range:true,snapshots:[{timestamp:"2026-01-01",used_bytes:1,free_bytes:1,scanned_bytes:1}]})).toBe("INSUFFICIENT_RANGE");
    expect(timelineState({...base,snapshots:[{timestamp:"a",used_bytes:1,free_bytes:1,scanned_bytes:1}],report:{} as never})).toBe("READY");
  });
  it("renders exactly one point per real snapshot",()=>{
    const points=[1,3,2].map((used_bytes,index)=>({timestamp:String(index),used_bytes,free_bytes:0,scanned_bytes:0}));
    expect(historyPoints(points).split(" ")).toHaveLength(3);
  });
  it("supports and bounds custom time ranges",()=>{
    expect(clampTimelineRange(7)).toBe(7);
    expect(clampTimelineRange(-1)).toBe(1);
    expect(clampTimelineRange(9000)).toBe(3650);
  });
  it("routes findings into existing features",()=>{
    expect(timelineDestination("directory")).toBe("Storage");
    expect(timelineDestination("file")).toBe("Large Files");
    expect(timelineDestination("recover")).toBe("Space Rescue");
  });
});
