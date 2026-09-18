import type { TimelinePoint, TimelineResponse } from "../types";

export type TimelineState="EMPTY"|"FIRST_SNAPSHOT"|"INSUFFICIENT_RANGE"|"READY";
export type TimelineDestination="Storage"|"Large Files"|"Space Rescue";

export function timelineState(timeline:TimelineResponse|null):TimelineState{
  if(!timeline||timeline.snapshots.length===0)return "EMPTY";
  if(timeline.first_snapshot)return "FIRST_SNAPSHOT";
  if(timeline.insufficient_range||!timeline.report)return "INSUFFICIENT_RANGE";
  return "READY";
}

export function timelineDestination(kind:"directory"|"file"|"recover"):TimelineDestination{
  if(kind==="directory")return "Storage";
  if(kind==="file")return "Large Files";
  return "Space Rescue";
}

export function historyPoints(points:TimelinePoint[],width=700,height=180):string{
  if(!points.length)return "";
  const values=points.map(point=>point.used_bytes),minimum=Math.min(...values),maximum=Math.max(...values),span=Math.max(maximum-minimum,1);
  return points.map((point,index)=>{
    const x=points.length===1?width/2:index/(points.length-1)*width;
    const y=height-(point.used_bytes-minimum)/span*height;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
}

export function clampTimelineRange(days:number):number{
  return Math.min(Math.max(Number.isFinite(days)?Math.round(days):1,1),3650);
}
