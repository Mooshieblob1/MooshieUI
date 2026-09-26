/*
 * Tiny launcher for Tauri's cached linuxdeploy AppImage.
 *
 * Tauri runs `dd seek=8` on this file to hide it from AppImage desktop integration.
 * That zeros e_ident[8..10] in a type-2 AppImage and breaks extract-and-run on Arch.
 * As a normal ELF binary those bytes are harmless padding, so we delegate to the
 * unpatched copy at `<this-path>.real` instead.
 */
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
	(void)argc;
	static const char suffix[] = ".real";
	char self[PATH_MAX];
	/* Leave room for the suffix and its terminating NUL (sizeof counts both). */
	const size_t max_len = sizeof(self) - sizeof(suffix);
	ssize_t len = readlink("/proc/self/exe", self, max_len);
	if (len < 0) {
		perror("readlink(/proc/self/exe)");
		return 127;
	}
	/* readlink does not terminate, and a full buffer may mean a truncated path. */
	if ((size_t)len >= max_len) {
		fprintf(stderr, "linuxdeploy wrapper: executable path too long\n");
		return 127;
	}
	memcpy(self + len, suffix, sizeof(suffix));

	argv[0] = self;
	execv(self, argv);
	perror("execv(linuxdeploy.real)");
	return 127;
}
