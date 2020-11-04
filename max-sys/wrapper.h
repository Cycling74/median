
/************ max ***********/
#ifdef __APPLE__
//forward decl files for mac
#define __FILES__
struct FSRef;
typedef struct FSRef FSRef;
#endif

//for t_critical
struct  OpaqueMPCriticalRegionID;
typedef struct OpaqueMPCriticalRegionID*  MPCriticalRegionID;

#include "wrapper-max.h"

/*********** msp ***********/
#include <z_dsp.h>
#include <ext_buffer.h>
#include <r_pfft.h>

/*********** jitter ***********/
/* TODO
typedef uint32_t CGDirectDisplayID;
typedef uint16_t GLhalfNV;
#include "wrapper-jitter.h"
*/
