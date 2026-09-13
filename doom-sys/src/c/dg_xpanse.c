/* Port glue for running doomgeneric on the Hackxpansion console (RP2354B,
 * Cortex-M33 @ 150 MHz, 320x200 RGB565 display, 4-button input).
 */
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdarg.h>

#include "config.h"
#include "doomtype.h"
#include "doomgeneric.h"
#include "i_system.h"
#include "i_video.h"
#include "w_wad.h"
#include "z_zone.h"
#include "p_mobj.h"
#include "g_game.h"
#include <setjmp.h>

/* Rust functions - implemented in doom-sys/src/lib.rs */
extern void dg_log_str(const unsigned char *s, unsigned int len);
extern uint32_t dg_get_ticks_ms(void);
extern void dg_sleep_ms(unsigned int ms);
extern void dg_push_key(int pressed, unsigned char key);
extern void dg_set_wad(const unsigned char *data, unsigned int size);
extern void *dg_heap_alloc(unsigned int size);
extern void *dg_heap_alloc_zeroed(unsigned int size);
extern void dg_heap_free(void *ptr, unsigned int size);
extern unsigned int dg_heap_alloc_size(void *ptr);

/* C functions for target (defined in Rust) */
extern int dg_bench;
extern unsigned int dg_bench_ms;
extern void dg_set_bench(int enabled);
extern void dg_fatal(const char *msg);

/* Platform v-table from C (for C functions) and Rust (for Rust functions) */
extern const struct DgPlatform DG_PLATFORM;
static const struct DgPlatform *platform = &DG_PLATFORM;

#if defined(DG_ZONE_SIZE)
static byte dg_zone[DG_ZONE_SIZE] __attribute__((aligned(8)));
#else
#error "DG_ZONE_SIZE must be defined"
#endif

const byte *dg_wad_data = NULL;
unsigned int dg_wad_size = 0;

static void dg_log(const char *s)
{
    if (s != NULL)
        platform->log_str(s, strlen(s));
}

jmp_buf dg_jmp_buf;

void dg_fatal(const char *msg)
{
    platform->log_str("DOOM fatal: ", 12);
    dg_log(msg);
    platform->log_str("\n", 1);
    longjmp(dg_jmp_buf, 1);
}

/* --- DOOM platform callbacks ------------------------------------------------ */

/* Key events are queued by Rust dg_push_key and consumed here. */
extern int dg_pop_key(int *pressed, unsigned char *key);

int DG_GetKey(int *pressed, unsigned char *key)
{
    return dg_pop_key(pressed, key);
}

void DG_Init(void)
{
    /* I_VideoBuffer and the palette are set up later by I_InitGraphics /
     * I_SetPalette, both of which run from D_DoomMain. */
}

void DG_DrawFrame(void)
{
    /* xpanse: frames are blitted by the Rust driver right after the tick
     * returns, so nothing needs to happen here. */
}

void DG_SleepMs(uint32_t ms)
{
    if (dg_bench)
        return;
    platform->sleep_ms(ms);
}

uint32_t DG_GetTicksMs(void)
{
    if (dg_bench)
    {
        /* One 35 Hz tic of virtual time per frame. */
        dg_bench_ms += 1000 / 35;
        return dg_bench_ms;
    }
    return platform->get_ticks_ms();
}

void DG_SetWindowTitle(const char *title)
{
    (void)title;
}

/* --- run helpers ------------------------------------------------------------- */

/* Runs exactly one DOOM frame (tic update + render + DG_DrawFrame).
 * Returns 1 if DOOM raised a fatal error (I_Error/exit), 0 otherwise. */
int dg_run_tick(void)
{
    int result = 1;
    if (setjmp(dg_jmp_buf) == 0)
    {
        doomgeneric_Tick();
        result = 0;
    }
    return result;
}

/* Boots the engine. D_DoomMain runs synchronously and performs all
 * initialization; it returns after one display tick (doomgeneric's patched
 * D_DoomLoop). */
static char *dg_argv[] =
{
    (char *)"doom", "-iwad", (char *)"doom1.wad",
    (char *)"-warp", (char *)"1", (char *)"1",
    (char *)"-skill", (char *)"3",
};

int dg_start(const byte *wad, unsigned int size)
{
    int result = 1;
    platform->set_wad(wad, size);
    if (setjmp(dg_jmp_buf) == 0)
    {
        doomgeneric_Create(8, dg_argv);
        result = 0;
    }
    return result;
}

/* Zone base for i_system.c. */
byte *dg_zone_base(int *size)
{
    *size = DG_ZONE_SIZE;
    return dg_zone;
}

/* Stub: the console shows no ENDOOM screen; I_Quit just returns. */
void I_Endoom(byte *endoom)
{
    (void)endoom;
}

/* --- disabled subsystems (RAM/flash budget: automap, intermission, status
 * bar, HUD messages, save games) ------------------------------------------------
 * These are no-ops on the console; the corresponding WAD lumps are not
 * embedded. The vanilla code paths still call them, so the symbols must
 * exist. */

void AM_Init(void) {}
void AM_Stop(void) {}
void AM_Start(void) {}
void AM_Ticker(void) {}
void AM_Drawer(void) {}
boolean AM_Responder(event_t *ev) { (void)ev; return false; }

void WI_Start(void) {}
void WI_End(void) {}
void WI_Ticker(void) { extern void G_WorldDone(void); static int t; if (++t > 2 * 35) G_WorldDone(); }
void WI_Drawer(void) {}
char WI_Responder(event_t *ev) { (void)ev; return 0; }

void ST_Start(void) {}
void ST_Stop(void) {}
void ST_Init(void) {}
void ST_Ticker(void) {}
void ST_Drawer(boolean status, boolean refresh) { (void)status; (void)refresh; }
void STlib_init(void) {}
boolean ST_Responder(event_t *ev) { (void)ev; return false; }

void HU_Init(void) {}
void HU_Start(void) {}
void HU_Ticker(void) {}
void HU_Drawer(void) {}
void HU_Erase(void) {}
boolean HU_Responder(event_t *ev) { (void)ev; return false; }

/* Multiplayer leftovers (hu_stuff is not linked). */
char *player_names[] = { "Player 1", "Player 2", "Player 3", "Player 4" };
void M_Init(void) {}
void M_Ticker(void) {}
boolean M_Responder(event_t *ev) { (void)ev; return false; }
void M_Drawer(void) {}
void M_StartControlPanel(void) {}
void M_ForceMenuOff(void) {}
void M_ClearMenus(void) {}
void M_ToggleMenu(void) {}

/* Globals that lived in the stubbed-out subsystems. */
int menuactive = 0;
int automapactive = 0;
int inhelpscreens = 0;
int showMessages = 1;
int screenblocks = 11; /* 11 = fullscreen view (320x200), no status bar */
int detailLevel = 0;
int mouseSensitivity = 5;
char *chat_macros[] = { "", "", "", "" };
void *hu_font[16];
char HU_dequeueChatChar(void) { return 0; }
/* F_finale is not linked (E1M9 is not embedded). */
void F_StartCast(void) {}
void F_Ticker(void) {}
void F_Drawer(void) {}
void F_StartFinale(void) {}
void F_Responder(event_t *ev) { (void)ev; }
/* m_config is not linked. */
char *M_GetSaveGameDir(char *iwadname) { (void)iwadname; return ""; }
void M_SaveDefaults(void) {}
void M_LoadDefaults(void) {}
void M_SetConfigFilenames(char *def, char *game) { (void)game; (void)def; }
void M_SetConfigDir(char *dir) { (void)dir; }
void M_BindVariable(char *name, void *location) { (void)name; (void)location; }
void M_BindVariableStr(char *name, void *location) { M_BindVariable(name, location); }
/* Save games are not supported (m_menu is stubbed); p_saveg is not linked. */
char *P_TempSaveGameFile(void) { return ""; }
char *P_SaveGameFile(int slot) { (void)slot; return ""; }
void P_WriteSaveGameHeader(char *desc) { (void)desc; }
int P_ReadSaveGameHeader(void) { return 1; }
void P_WriteSaveGameEOF(void) {}
int P_ReadSaveGameEOF(void) { return 1; }
void P_ArchivePlayers(void) {}
void P_ArchiveWorld(void) {}
void P_ArchiveThinkers(void) {}
void P_ArchiveSpecials(void) {}
void P_UnArchivePlayers(void) {}
void P_UnArchiveWorld(void) {}
void P_UnArchiveThinkers(void) {}
void P_UnArchiveSpecials(void) {}
void P_UnArchiveSpecials2(void) {}
void StatCopy(unsigned int *stats) { (void)stats; }
void StatDump(void) {}
void W_Checksum(int *dest) { (void)dest; }
unsigned int W_GetChecksum(void) { return 0; }
FILE *save_stream;
boolean savegame_error;
/* The sound/music subsystem is not linked (the console has no audio). */
void S_Init(int sfxVolume, int musicVolume) { (void)sfxVolume; (void)musicVolume; }
void S_Start(void) {}
void S_StopSound(mobj_t *origin) { (void)origin; }
void S_StartSound(void *origin, int sfx_id) { (void)origin; (void)sfx_id; }
void S_PauseSound(void) {}
void S_ResumeSound(void) {}
void S_UpdateSounds(mobj_t *listener) { (void)listener; }
void S_SetMusicVolume(int volume) { (void)volume; }
void S_SetSfxVolume(int volume) { (void)volume; }
void S_StartMusic(int m_id) { (void)m_id; }
void S_ChangeMusic(int musicnum, int looping) { (void)musicnum; (void)looping; }
void S_StopMusic(void) {}
int cht_CheckCheat(char *cheat, char c) { (void)cheat; (void)c; return 0; }
void cht_GetParam(char *cheat, char *buf) { (void)cheat; (void)buf; }
int sfxVolume;
int musicVolume;
int snd_channels;

/* Stubs for config and filesystem pieces that are not linked on console. */
void I_BindJoystickVariables(void) {}

void I_InitInput(void) {}
event_t *I_GetEvent(void) { return NULL; }
void I_InitJoystick(void) {}

/* Platform v-table for target (C implementation for C functions, Rust for Rust functions) */
const struct DgPlatform DG_PLATFORM = {
    .log_str = dg_log_str,
    .get_ticks_ms = dg_get_ticks_ms,
    .sleep_ms = dg_sleep_ms,
    .fatal = dg_fatal,
    .set_bench = dg_set_bench,
    .push_key = dg_push_key,
    .set_wad = dg_set_wad,
    .heap_alloc = dg_heap_alloc,
    .heap_alloc_zeroed = dg_heap_alloc_zeroed,
    .heap_free = dg_heap_free,
    .heap_alloc_size = dg_heap_alloc_size,
};
