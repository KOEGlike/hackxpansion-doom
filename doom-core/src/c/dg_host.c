/* Host implementations of engine runner functions and stubs for testing */

#include "doomgeneric.h"

#include "doomtype.h"
#include "doomdef.h"
#include "d_event.h"
#include "m_misc.h"
#include "p_mobj.h"
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>

/* Forward declarations for engine functions */
void DG_Init(void);
void DG_DrawFrame(void);
void doomgeneric_Create(int argc, char **argv);

/* Platform functions are provided by doom-sys */
extern void dg_log_str(const char *s, unsigned int len);
extern uint32_t dg_get_ticks_ms(void);
extern void dg_sleep_ms(uint32_t ms);
extern void dg_fatal_rust(const char *msg);
extern void dg_set_bench(int enabled);
extern int dg_bench;
extern unsigned int dg_bench_ms;
extern void dg_push_key(int pressed, unsigned char key);
extern void dg_set_wad(const unsigned char *data, unsigned int size);
extern void *dg_heap_alloc(unsigned int size);
extern void *dg_heap_alloc_zeroed(unsigned int size);
extern void dg_heap_free(void *ptr, unsigned int size);
extern unsigned int dg_heap_alloc_size(void *ptr);

/* Static variables for bench */

/* Static variables for WAD */
const unsigned char *dg_wad_data = NULL;
unsigned int dg_wad_size = 0;

/* Zone base - provided by i_system.c */
static byte dg_zone[DG_ZONE_SIZE] __attribute__((aligned(8)));

/* Jump buffer for fatal errors */
jmp_buf dg_jmp_buf;

/* Key events are queued by Rust dg_push_key and consumed here. */
extern int dg_pop_key(int *pressed, unsigned char *key);

/* DG_* platform callbacks - call through to dg_* functions from doom-sys */
void DG_Init(void) {}

void DG_DrawFrame(void) {
    /* xpanse: frames are blitted by the Rust driver right after the tick
     * returns, so nothing needs to happen here. */
}

void DG_SleepMs(uint32_t ms) {
    if (dg_bench)
        return;
    dg_sleep_ms(ms);
}

uint32_t DG_GetTicksMs(void) {
    if (dg_bench) {
        /* One 35 Hz tic of virtual time per frame. */
        dg_bench_ms += 1000 / 35;
        return dg_bench_ms;
    }
    return dg_get_ticks_ms();
}

void DG_SetWindowTitle(const char *title) {
    (void)title;
}

/* Fatal errors on host are reported through the Rust dg_fatal v-table
 * entry (see doom-sys/src/lib.rs); no C dg_fatal is defined here so the
 * Rust symbol remains the unique definition. */

int DG_GetKey(int *pressed, unsigned char *key) {
    return dg_pop_key(pressed, key);
}

/* Engine entry points */
int dg_start(const byte *wad, unsigned int size) {
    dg_set_wad(wad, size);
    /* xpanse: host test uses the same startup as target */
    static char *dg_argv[] = {
        (char *)"doom", "-iwad", (char *)"doom1.wad",
        (char *)"-warp", (char *)"1", (char *)"1",
        (char *)"-skill", (char *)"3",
    };
    int result = 1;
    if (setjmp(dg_jmp_buf) == 0) {
        doomgeneric_Create(8, dg_argv);
        result = 0;
    }
    return result;
}

int dg_run_tick(void) {
    int result = 1;
    if (setjmp(dg_jmp_buf) == 0) {
        doomgeneric_Tick();
        result = 0;
    }
    return result;
}

/* Zone base accessor */
byte *dg_zone_base(int *size) {
    *size = DG_ZONE_SIZE;
    return dg_zone;
}

/* Zone telemetry for RAM budgeting (host tests only). */
void Z_ZoneStats(int *s, int *l, int *c, int *f);
void dg_zone_stats(int *s, int *l, int *c, int *f) {
    Z_ZoneStats(s, l, c, f);
}
void Z_ZoneTop(void);
void dg_zone_top(void) {
    Z_ZoneTop();
}

/* Stubs for disabled subsystems - from dg_xpanse.c */
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

/* I_Endoom stub */
void I_Endoom(byte *endoom) { (void)endoom; }

/* Input/joystick stubs (mirrors the dg_xpanse.c target glue; i_joystick.c
 * is not linked on the console). */
void I_InitInput(void) {}
event_t *I_GetEvent(void) { return NULL; }
void I_InitJoystick(void) {}
void I_BindJoystickVariables(void) {}
