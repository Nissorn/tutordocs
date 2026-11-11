// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import {
  createCourse,
  emptyTrash,
  listLibraryCourses,
  listTrash,
  markCourseOpened,
  renameCourse,
  restoreCourse,
  setCourseTags,
  trashCourse,
} from "./courses-api";

afterEach(() => {
  clearMocks();
});

type Sent = { cmd: string; args: Record<string, unknown> };

function backend(replies: Record<string, unknown>): Sent[] {
  const sent: Sent[] = [];
  mockIPC((cmd, raw) => {
    sent.push({ cmd, args: (raw ?? {}) as Record<string, unknown> });
    if (!(cmd in replies)) throw new Error(`unexpected command ${cmd}`);
    return replies[cmd];
  });
  return sent;
}

const fakeCourse = {
  id: "biology",
  storedId: "biology-orig",
  title: "Cell Biology",
  tags: ["science"],
  objectives: ["Cells"],
  chapterCount: 2,
  chapterTitles: ["Cells", "Membranes"],
  writtenChapterCount: 1,
  createdAt: 1700000000,
  openedAt: null,
  error: null,
  outlineStatus: "approved",
  planning: false,
};

const fakeTrash = {
  trashId: "biology-1700000000",
  courseId: "biology",
  title: "Cell Biology",
  trashedAt: "2026-11-09T12:00:00Z",
  chapterCount: 2,
  writtenChapterCount: 1,
  bytes: 54321,
};

describe("courses-api", () => {
  it("listLibraryCourses calls list_library_courses and returns parsed courses", async () => {
    const sent = backend({ list_library_courses: [fakeCourse] });
    const result = await listLibraryCourses();
    expect(sent).toEqual([{ cmd: "list_library_courses", args: {} }]);
    expect(result).toEqual([fakeCourse]);
  });

  it("trashCourse calls trash_course with camelCase courseId and returns TrashEntry", async () => {
    const sent = backend({ trash_course: fakeTrash });
    const result = await trashCourse("biology");
    expect(sent).toEqual([{ cmd: "trash_course", args: { courseId: "biology" } }]);
    expect(result).toEqual(fakeTrash);
  });

  it("listTrash calls list_trash and returns TrashEntry[]", async () => {
    const sent = backend({ list_trash: [fakeTrash] });
    const result = await listTrash();
    expect(sent).toEqual([{ cmd: "list_trash", args: {} }]);
    expect(result).toEqual([fakeTrash]);
  });

  it("restoreCourse calls restore_course with camelCase trashId and returns LibraryCourse", async () => {
    const sent = backend({ restore_course: fakeCourse });
    const result = await restoreCourse("biology-1700000000");
    expect(sent).toEqual([{ cmd: "restore_course", args: { trashId: "biology-1700000000" } }]);
    expect(result).toEqual(fakeCourse);
  });

  it("emptyTrash calls empty_trash with camelCase trashId", async () => {
    const sent = backend({ empty_trash: null });
    await emptyTrash("biology-1700000000");
    expect(sent).toEqual([{ cmd: "empty_trash", args: { trashId: "biology-1700000000" } }]);
  });

  it("renameCourse calls rename_course with camelCase courseId and title and returns LibraryCourse", async () => {
    const renamed = { ...fakeCourse, title: "Advanced Cell Biology" };
    const sent = backend({ rename_course: renamed });
    const result = await renameCourse("biology", "Advanced Cell Biology");
    expect(sent).toEqual([{ cmd: "rename_course", args: { courseId: "biology", title: "Advanced Cell Biology" } }]);
    expect(result.title).toBe("Advanced Cell Biology");
  });
});
