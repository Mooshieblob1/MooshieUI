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
	char self[PATH_MAX];
	ssize_t len = readlink("/proc/self/exe", self, sizeof(self) - 5);
	if (len < 0) {
		perror("readlink(/proc/self/exe)");
		return 127;
	}
	self[len] = '\0';
	strcat(self, ".real");

	argv[0] = self;
	execv(self, argv);
	perror("execv(linuxdeploy.real)");
	return 127;
}
