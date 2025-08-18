
/************ max ***********/
#ifdef __APPLE__
//forward decl files for mac
#define __FILES__
struct FSRef;
typedef struct FSRef FSRef;
#endif

//va_list
#include <stdarg.h>

//for t_critical
struct  OpaqueMPCriticalRegionID;
typedef struct OpaqueMPCriticalRegionID*  MPCriticalRegionID;

#include "wrapper-max.h"

/*********** msp ***********/
#include <z_dsp.h>
#include <ext_buffer.h>
#include <r_pfft.h>

#undef __FILES__

/*********** jitter ***********/

// the public headers don't seem to support linux yet, ignoring jitter for now
#ifndef LINUX_VERSION
//typedef uint32_t CGDirectDisplayID;
//typedef uint16_t GLhalfNV;
#include "wrapper-jitter.h"
#endif
