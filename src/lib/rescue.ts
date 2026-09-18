import type { DuplicateGroup, FileEntry, Recommendation } from "../types";

export type RescueDestination="Duplicates"|"Cleanup"|"Large Files";

export function normalizeRescueTargetGb(value:number):number{
  return Math.min(Math.max(Number.isFinite(value)?value:1,1),100000);
}

export function rescueDestination(action:string):RescueDestination{
  if(action==="review_duplicates")return "Duplicates";
  if(action==="review_cleanup")return "Cleanup";
  return "Large Files";
}

export function rescueProgress(initialNeed:number,recovered:number){
  const safeNeed=Math.max(initialNeed,0),safeRecovered=Math.max(recovered,0);
  return {
    percent:safeNeed?Math.min(safeRecovered/safeNeed*100,100):100,
    remaining:Math.max(safeNeed-safeRecovered,0),
  };
}

export function filterRescueItems(files:FileEntry[],groups:DuplicateGroup[],tips:Recommendation[],paths:Set<string>|null){
  if(!paths)return {files,groups,tips};
  return {
    files:files.filter(file=>paths.has(file.path)),
    groups:groups.filter(group=>group.files.some(file=>paths.has(file.path))),
    tips:tips.filter(tip=>paths.has(tip.file.path)),
  };
}
