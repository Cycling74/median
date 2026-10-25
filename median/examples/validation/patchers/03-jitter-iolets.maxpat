{
  "patcher": {
    "fileversion": 1,
    "appversion": {
      "major": 9,
      "minor": 0,
      "revision": 0,
      "architecture": "x64",
      "modernui": 1
    },
    "classnamespace": "box",
    "rect": [
      50.0,
      50.0,
      1000.0,
      760.0
    ],
    "default_fontsize": 13.0,
    "default_fontface": 0,
    "default_fontname": "Arial",
    "boxes": [
      {
        "box": {
          "id": "title",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            20.0,
            900.0,
            40.0
          ],
          "text": "3. Jitter custom iolets \u2014 positions are zero-based, including SDK outlets and dumpout."
        }
      },
      {
        "box": {
          "id": "matrixhelp",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            65.0,
            900.0,
            60.0
          ],
          "text": "MOP: matrix inlets 0\u20131; custom integer 2 (cold), float 3 (hot), proxy 4 (cold). Matrix outlets 0\u20131; dumpout 2; integer echo 3; probe bang 4. This fixture does not process matrices."
        }
      },
      {
        "box": {
          "id": "mi",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            380.0,
            150.0,
            60.0,
            22.0
          ],
          "text": "19",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "mf",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            550.0,
            150.0,
            60.0,
            22.0
          ],
          "text": "2.5",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "mp",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            720.0,
            150.0,
            90.0,
            22.0
          ],
          "text": "probe",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "matrix",
          "maxclass": "newobj",
          "numinlets": 5,
          "numoutlets": 5,
          "patching_rect": [
            40.0,
            220.0,
            780.0,
            22.0
          ],
          "text": "jit.median.validate",
          "outlettype": [
            "jit_matrix",
            "jit_matrix",
            "",
            "int",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "dump",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            330.0,
            285.0,
            150.0,
            22.0
          ],
          "text": "print mop-dump",
          "outlettype": []
        }
      },
      {
        "box": {
          "id": "echo",
          "maxclass": "number",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            585.0,
            285.0,
            70.0,
            22.0
          ],
          "outlettype": [
            "int",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "done",
          "maxclass": "button",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            790.0,
            285.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "expectm",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            330.0,
            900.0,
            65.0
          ],
          "text": "Expected: echo displays 19; console reports float inlet 3 value 2.5 and proxy inlet 4; button flashes. Hover SDK/custom iolets; inspect hot/cold colors. Also create jit.median.validate refuse manually: it should report an intentional error, then permit normal creation."
        }
      },
      {
        "box": {
          "id": "gp",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            425.0,
            435.0,
            90.0,
            22.0
          ],
          "text": "probe",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "gl",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 3,
          "patching_rect": [
            40.0,
            490.0,
            500.0,
            22.0
          ],
          "text": "jit.gl.median.validate median_validation",
          "outlettype": [
            "jit_matrix",
            "",
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "gi",
          "maxclass": "number",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            470.0,
            550.0,
            70.0,
            22.0
          ],
          "outlettype": [
            "int",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "enable",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            625.0,
            435.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "pe",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            670.0,
            435.0,
            150.0,
            22.0
          ],
          "text": "prepend enable",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "world",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 3,
          "patching_rect": [
            600.0,
            490.0,
            350.0,
            22.0
          ],
          "text": "jit.world median_validation @enable 0 @visible 0",
          "outlettype": [
            "jit_matrix",
            "bang",
            ""
          ]
        }
      },
      {
        "box": {
          "id": "show",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            650.0,
            550.0,
            110.0,
            22.0
          ],
          "text": "visible 1",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "hide",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            800.0,
            550.0,
            110.0,
            22.0
          ],
          "text": "visible 0",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "expectg",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            610.0,
            900.0,
            65.0
          ],
          "text": "GL: one SDK inlet 0, hot custom proxy 1; SDK matrix outlet 0, dumpout 1, custom index outlet 2. probe should display 1 and report proxy inlet 1. Enable the context and repeat. This object draws nothing; a blank window is expected."
        }
      },
      {
        "box": {
          "id": "lifecycle",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            690.0,
            900.0,
            50.0
          ],
          "text": "Stop rendering; delete/recreate the GL object and context, including deleting the object after its context. For actual drawing use jit_gl_simple as listed in doc/validation-max.md."
        }
      }
    ],
    "lines": [
      {
        "patchline": {
          "source": [
            "mi",
            0
          ],
          "destination": [
            "matrix",
            2
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "mf",
            0
          ],
          "destination": [
            "matrix",
            3
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "mp",
            0
          ],
          "destination": [
            "matrix",
            4
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "matrix",
            2
          ],
          "destination": [
            "dump",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "matrix",
            3
          ],
          "destination": [
            "echo",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "matrix",
            4
          ],
          "destination": [
            "done",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "gp",
            0
          ],
          "destination": [
            "gl",
            1
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "gl",
            2
          ],
          "destination": [
            "gi",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "enable",
            0
          ],
          "destination": [
            "pe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "pe",
            0
          ],
          "destination": [
            "world",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "show",
            0
          ],
          "destination": [
            "world",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "hide",
            0
          ],
          "destination": [
            "world",
            0
          ]
        }
      }
    ]
  }
}
