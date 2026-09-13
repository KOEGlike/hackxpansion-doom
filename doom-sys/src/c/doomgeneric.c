#include <stdio.h>

#include "m_argv.h"

#include "doomgeneric.h"

pixel_t* DG_ScreenBuffer = NULL;

void M_FindResponseFile(void);
void D_DoomMain (void);


void doomgeneric_Create(int argc, char **argv)
{
	// save arguments
    myargc = argc;
    myargv = argv;

	M_FindResponseFile();

	/* xpanse: no intermediate screen buffer on the console; the Rust
	 * blit reads I_VideoBuffer directly. */
	(void)0;

	DG_Init();

	D_DoomMain ();
}

