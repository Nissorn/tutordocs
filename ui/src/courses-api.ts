import { invoke } from "@tauri-apps/api/core";
import { list } from "./api";
import { parseCourse, parseTrashEntry, type LibraryCourse, type TrashEntry } from "./courses";

// The typed bridge to the shelf commands in src-tauri/src/reader.rs.

export async function listLibraryCourses(): Promise<LibraryCourse[]> {
  return list(await invoke<unknown>("list_library_courses"), "list_library_courses").map(parseCourse);
}

export async function createCourse(title: string, tags: string[]): Promise<LibraryCourse> {
  return parseCourse(await invoke<unknown>("create_course", { title, tags }));
}

export async function setCourseTags(courseId: string, tags: string[]): Promise<LibraryCourse> {
  return parseCourse(await invoke<unknown>("set_course_tags", { courseId, tags }));
}

/** Also makes this the course that commands without a course id act on. */
export async function markCourseOpened(courseId: string): Promise<LibraryCourse> {
  return parseCourse(await invoke<unknown>("mark_course_opened", { courseId }));
}

export async function trashCourse(courseId: string): Promise<TrashEntry> {
  return parseTrashEntry(await invoke<unknown>("trash_course", { courseId }));
}

export async function listTrash(): Promise<TrashEntry[]> {
  return list(await invoke<unknown>("list_trash"), "list_trash").map(parseTrashEntry);
}

export async function restoreCourse(trashId: string): Promise<LibraryCourse> {
  return parseCourse(await invoke<unknown>("restore_course", { trashId }));
}

export async function emptyTrash(trashId: string): Promise<void> {
  await invoke<void>("empty_trash", { trashId });
}

export async function renameCourse(courseId: string, title: string): Promise<LibraryCourse> {
  return parseCourse(await invoke<unknown>("rename_course", { courseId, title }));
}

