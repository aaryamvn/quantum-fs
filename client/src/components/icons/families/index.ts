import type { JSX } from "react";

import { Archive } from "./Archive";
import { Audio } from "./Audio";
import { Book } from "./Book";
import { Certificate } from "./Certificate";
import { Code } from "./Code";
import { Config } from "./Config";
import { Data } from "./Data";
import { Database } from "./Database";
import { Design } from "./Design";
import { Disk } from "./Disk";
import { Document } from "./Document";
import { Executable } from "./Executable";
import { Font } from "./Font";
import { Generic } from "./Generic";
import { Image } from "./Image";
import { Model3d } from "./Model3d";
import { Pdf } from "./Pdf";
import { Presentation } from "./Presentation";
import { Spreadsheet } from "./Spreadsheet";
import { Text } from "./Text";
import { Vector } from "./Vector";
import { Video } from "./Video";
import { Web } from "./Web";
import type { IconFamily, IconFamilyProps } from "../types";

/**
 * The one place a family name becomes a drawing.
 *
 * `Record<IconFamily, …>` is load-bearing: adding a name to `IconFamily`
 * without adding a row here is a compile error, so the map can never drift
 * behind the type. Each family owns exactly one file, which is what lets
 * several people draw different families at the same time — this table is
 * written once and then left alone.
 */
export const FAMILY_COMPONENTS: Record<IconFamily, (props: IconFamilyProps) => JSX.Element | null> =
  {
    document: Document,
    text: Text,
    code: Code,
    data: Data,
    image: Image,
    vector: Vector,
    video: Video,
    audio: Audio,
    archive: Archive,
    spreadsheet: Spreadsheet,
    presentation: Presentation,
    pdf: Pdf,
    font: Font,
    model3d: Model3d,
    disk: Disk,
    executable: Executable,
    database: Database,
    design: Design,
    config: Config,
    web: Web,
    book: Book,
    certificate: Certificate,
    generic: Generic,
  };
