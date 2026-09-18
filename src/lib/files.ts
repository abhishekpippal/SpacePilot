import type { FileEntry } from "../types";

export type FileSort = "size" | "name" | "modified";

export function filterAndSortFiles(files: FileEntry[], minimumBytes: number, sort: FileSort): FileEntry[] {
  return files
    .filter(file => file.size >= minimumBytes)
    .slice()
    .sort((left, right) => {
      if (sort === "name") return left.name.localeCompare(right.name);
      if (sort === "modified") return (Date.parse(right.modified ?? "") || 0) - (Date.parse(left.modified ?? "") || 0);
      return right.size - left.size;
    });
}
