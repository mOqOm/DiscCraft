//! The XMP schemas and properties the Metadata panel lists, in Encore CS6's order (M6.397; docs/encore-behavior.md
//! 9.14): every schema, and every standard property of it, whether the file has it or not. The Japanese names Encore
//! shows are in the UI catalog (`ja.tsv`): a schema's under the context `xmp`, its properties' under `xmp:<prefix>`
//! (a test checks that each one has its entry).

/// How a property is shown and written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    /// A simple text value, edited in a box.
    Text,
    /// A language alternative (`rdf:Alt` of `xml:lang` items): its default item is edited.
    LanguageAlternative,
    /// An ordered list (`rdf:Seq`), edited as items separated by ";".
    Ordered,
    /// An unordered list (`rdf:Bag`), edited as items separated by ";".
    Unordered,
    /// True or false (`True` / `False`), edited with a check box.
    Boolean,
    /// A rating: -1 (rejected) or 0 to 5 stars, chosen from a list.
    Rating,
    /// A value shown only.
    Shown,
    /// True or false, shown only.
    ShownBoolean,
    /// A structure or an array of structures, its fields shown under an opener.
    Structure,
}

impl PropertyKind {
    /// Whether the panel edits it.
    #[must_use]
    pub fn editable(self) -> bool {
        !matches!(
            self,
            PropertyKind::Shown | PropertyKind::ShownBoolean | PropertyKind::Structure
        )
    }
}

/// One property of a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertySpec {
    /// Its local name in the schema's namespace.
    pub local: &'static str,
    /// Its name in the panel (English; the UI catalog has Encore's Japanese one under the context `xmp:<prefix>`).
    pub label: &'static str,
    /// How it is shown and written.
    pub kind: PropertyKind,
}

/// One schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaSpec {
    /// Its namespace.
    pub namespace: &'static str,
    /// The usual prefix (written for a property it does not have yet).
    pub prefix: &'static str,
    /// Its name in the panel (English; Japanese in the catalog).
    pub name: &'static str,
    /// Encore shows its heading a little faint.
    pub faint: bool,
    /// Its properties, in the panel's order.
    pub properties: &'static [PropertySpec],
}

/// The schemas, in the panel's order.
pub const SCHEMAS: &[SchemaSpec] = &[
    SchemaSpec {
        namespace: "http://purl.org/dc/elements/1.1/",
        prefix: "dc",
        name: "Dublin Core",
        faint: false,
        properties: &[
            PropertySpec {
                local: "contributor",
                label: "Contributor",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "coverage",
                label: "Coverage",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "creator",
                label: "Creator",
                kind: PropertyKind::Ordered,
            },
            PropertySpec {
                local: "date",
                label: "Date",
                kind: PropertyKind::Ordered,
            },
            PropertySpec {
                local: "description",
                label: "Description",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "format",
                label: "Format",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "identifier",
                label: "Identifier",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "publisher",
                label: "Publisher",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "relation",
                label: "Relation",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "rights",
                label: "Rights",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "source",
                label: "Source",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "subject",
                label: "Subject",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "title",
                label: "Title",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "type",
                label: "Type",
                kind: PropertyKind::Unordered,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xap/1.0/",
        prefix: "xmp",
        name: "Basic",
        faint: false,
        properties: &[
            PropertySpec {
                local: "Advisory",
                label: "Advisory",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "BaseURL",
                label: "Base URL",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "CreateDate",
                label: "Create Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "CreatorTool",
                label: "Creator Tool",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Identifier",
                label: "Identifier",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "Label",
                label: "Label",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "MetadataDate",
                label: "Metadata Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ModifyDate",
                label: "Modify Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Nickname",
                label: "Nickname",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Rating",
                label: "Rating",
                kind: PropertyKind::Rating,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xap/1.0/rights/",
        prefix: "xmpRights",
        name: "Rights Management",
        faint: false,
        properties: &[
            PropertySpec {
                local: "Certificate",
                label: "Certificate",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Marked",
                label: "Marked",
                kind: PropertyKind::Boolean,
            },
            PropertySpec {
                local: "Owner",
                label: "Owner",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "UsageTerms",
                label: "Usage Terms",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "WebStatement",
                label: "Web Statement",
                kind: PropertyKind::Text,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xap/1.0/mm/",
        prefix: "xmpMM",
        name: "Media Management",
        faint: false,
        properties: &[
            PropertySpec {
                local: "DocumentID",
                label: "Document ID",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "InstanceID",
                label: "Instance ID",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Manager",
                label: "Manager",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ManageTo",
                label: "Manage To",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ManageUI",
                label: "Manage UI",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ManagerVariant",
                label: "Manager Variant",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "RenditionClass",
                label: "Rendition Class",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "RenditionParams",
                label: "Rendition Params",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "VersionID",
                label: "Version ID",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "LastURL",
                label: "Last URL",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SaveID",
                label: "Save ID",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xap/1.0/bj/",
        prefix: "xmpBJ",
        name: "Basic Job Ticket",
        faint: false,
        properties: &[PropertySpec {
            local: "JobRef",
            label: "Job Reference",
            kind: PropertyKind::Structure,
        }],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xap/1.0/t/pg/",
        prefix: "xmpTPg",
        name: "Paged-Text",
        faint: false,
        properties: &[
            PropertySpec {
                local: "MaxPageSize",
                label: "Max Page Size",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "NPages",
                label: "Number of Pages",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/xmp/1.0/DynamicMedia/",
        prefix: "xmpDM",
        name: "Dynamic Media",
        faint: false,
        properties: &[
            PropertySpec {
                local: "projectRef",
                label: "Project Reference",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "videoFrameRate",
                label: "Video Frame Rate",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoFrameSize",
                label: "Video Frame Size",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "videoPixelAspectRatio",
                label: "Video Pixel Aspect Ratio",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoPixelDepth",
                label: "Video Pixel Depth",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoColorSpace",
                label: "Video Color Space",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoAlphaMode",
                label: "Video Alpha Mode",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoAlphaPremultipleColor",
                label: "Video Alpha Premultiple Color",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "videoAlphaUnityIsTransparent",
                label: "Video Alpha Unity Is Transparent",
                kind: PropertyKind::ShownBoolean,
            },
            PropertySpec {
                local: "videoCompressor",
                label: "Video Compressor",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoFieldOrder",
                label: "Video Field Order",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "pullDown",
                label: "Pull Down",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "audioSampleRate",
                label: "Audio Sample Rate",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "audioSampleType",
                label: "Audio Sample Type",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "audioChannelType",
                label: "Audio Channel Type",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "audioCompressor",
                label: "Audio Compressor",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "speakerPlacement",
                label: "Speaker Placement",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "fileDataRate",
                label: "File Data Rate",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "tapeName",
                label: "Tape Name",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "altTapeName",
                label: "Alternative Tape Name",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "startTimecode",
                label: "Start Timecode",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "altTimecode",
                label: "Alternative Timecode",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "duration",
                label: "Duration",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "scene",
                label: "Scene",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "shotName",
                label: "Shot Name",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "shotDate",
                label: "Shot Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "shotLocation",
                label: "Shot Location",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "logComment",
                label: "Log Comment",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "absPeakAudioFilePath",
                label: "Absolute Peak Audio File Path",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "relativePeakAudioFilePath",
                label: "Relative Peak Audio File Path",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "videoModDate",
                label: "Video Modified Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "audioModDate",
                label: "Audio Modified Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "metadataModDate",
                label: "Metadata Modified Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "artist",
                label: "Artist",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "album",
                label: "Album",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "trackNumber",
                label: "Track Number",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "genre",
                label: "Genre",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "releaseDate",
                label: "Release Date",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "composer",
                label: "Composer",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "engineer",
                label: "Engineer",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "tempo",
                label: "Tempo",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "instrument",
                label: "Instrument",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "introTime",
                label: "Intro Time",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "outCue",
                label: "Out Cue",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "relativeTimestamp",
                label: "Relative Timestamp",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "loop",
                label: "Loop",
                kind: PropertyKind::ShownBoolean,
            },
            PropertySpec {
                local: "numberOfBeats",
                label: "Number of Beats",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "key",
                label: "Key",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "stretchMode",
                label: "Stretch Mode",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "timeScaleParams",
                label: "Time Scale Parameters",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "resampleParams",
                label: "Resample Parameters",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "beatSpliceParams",
                label: "Beat Splice Parameters",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "timeSignature",
                label: "Time Signature",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "scaleType",
                label: "Scale Type",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        name: "Adobe PDF",
        faint: true,
        properties: &[
            PropertySpec {
                local: "Keywords",
                label: "Keywords",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "PDFVersion",
                label: "PDF Version",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Producer",
                label: "Producer",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/photoshop/1.0/",
        prefix: "photoshop",
        name: "Photoshop",
        faint: true,
        properties: &[
            PropertySpec {
                local: "AuthorsPosition",
                label: "Author's Position",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "CaptionWriter",
                label: "Caption Writer",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Category",
                label: "Category",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "City",
                label: "City",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Country",
                label: "Country",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Credit",
                label: "Credit",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "DateCreated",
                label: "Date Created",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Headline",
                label: "Headline",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Instructions",
                label: "Instructions",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Source",
                label: "Source",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "State",
                label: "State",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "SupplementalCategories",
                label: "Supplemental Categories",
                kind: PropertyKind::Unordered,
            },
            PropertySpec {
                local: "TransmissionReference",
                label: "Transmission Reference",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Urgency",
                label: "Urgency",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/exif/1.0/",
        prefix: "exif",
        name: "EXIF Schema for EXIF-specific Properties",
        faint: false,
        properties: &[
            PropertySpec {
                local: "ExifVersion",
                label: "Exif Version",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FlashpixVersion",
                label: "Flashpix Version",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ColorSpace",
                label: "Color Space",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "CompressedBitsPerPixel",
                label: "Compressed Bits Per Pixel",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "PixelXDimension",
                label: "Pixel X Dimension",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "PixelYDimension",
                label: "Pixel Y Dimension",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "UserComment",
                label: "User Comment",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "RelatedSoundFile",
                label: "Related Sound File",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "DateTimeOriginal",
                label: "Date Time Original",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "DateTimeDigitized",
                label: "Date Time Digitized",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ExposureTime",
                label: "Exposure Time",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FNumber",
                label: "F-Stop",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ExposureProgram",
                label: "Exposure Program",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SpectralSensitivity",
                label: "Spectral Sensitivity",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "OECF",
                label: "Opto-Electronic Conversion Function (OECF)",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "ShutterSpeedValue",
                label: "Shutter Speed",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ApertureValue",
                label: "Aperture",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "BrightnessValue",
                label: "Brightness",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ExposureBiasValue",
                label: "Exposure Bias",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "MaxApertureValue",
                label: "Max Aperture",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SubjectDistance",
                label: "Subject Distance",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "MeteringMode",
                label: "Metering Mode",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "LightSource",
                label: "Light Source",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Flash",
                label: "Flash",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "FocalLength",
                label: "Focal Length",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FlashEnergy",
                label: "Flash Energy",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SpatialFrequencyResponse",
                label: "Spatial Frequency Response",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "FocalPlaneXResolution",
                label: "Focal Plane X Resolution",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FocalPlaneYResolution",
                label: "Focal Plane Y Resolution",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FocalPlaneResolutionUnit",
                label: "Focal Plane Resolution Unit",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ExposureIndex",
                label: "Exposure Index",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SensingMethod",
                label: "Sensing Method",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FileSource",
                label: "File Source",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SceneType",
                label: "Scene Type",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "CFAPattern",
                label: "CFA Pattern",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "CustomRendered",
                label: "Custom Rendered",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ExposureMode",
                label: "Exposure Mode",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "WhiteBalance",
                label: "White Balance",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "DigitalZoomRatio",
                label: "Digital Zoom Ratio",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "FocalLengthIn35mmFilm",
                label: "Focal Length In 35mm Film",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SceneCaptureType",
                label: "Scene Capture Type",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GainControl",
                label: "Gain Control",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Contrast",
                label: "Contrast",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Saturation",
                label: "Saturation",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Sharpness",
                label: "Sharpness",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "DeviceSettingDescription",
                label: "Device Setting Description",
                kind: PropertyKind::Structure,
            },
            PropertySpec {
                local: "SubjectDistanceRange",
                label: "Subject Distance Range",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ImageUniqueID",
                label: "Image Unique ID",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSVersionID",
                label: "GPS Version ID",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSLatitude",
                label: "GPS Latitude",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSLongitude",
                label: "GPS Longitude",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSAltitudeRef",
                label: "GPS Altitude Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSAltitude",
                label: "GPS Altitude",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSTimeStamp",
                label: "GPS Time Stamp",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSSatellites",
                label: "GPS Satellites",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSStatus",
                label: "GPS Status",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSMeasureMode",
                label: "GPS Measure Mode",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDOP",
                label: "GPS DOP",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSSpeedRef",
                label: "GPS Speed Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSSpeed",
                label: "GPS Speed",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSTrackRef",
                label: "GPS Track Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSTrack",
                label: "GPS Track",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSImgDirectionRef",
                label: "GPS Image Direction Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSImgDirection",
                label: "GPS Image Direction",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSMapDatum",
                label: "GPS Map Datum",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestLatitude",
                label: "GPS Destination Latitude",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestLongitude",
                label: "GPS Destination Longitude",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestBearingRef",
                label: "GPS Destination Bearing Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestBearing",
                label: "GPS Destination Bearing",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestDistanceRef",
                label: "GPS Destination Distance Reference",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDestDistance",
                label: "GPS Destination Distance",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSProcessingMethod",
                label: "GPS Processing Method",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSAreaInformation",
                label: "GPS Area Information",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "GPSDifferential",
                label: "GPS Differential",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/exif/1.0/aux/",
        prefix: "aux",
        name: "EXIF Schema for Additional EXIF Properties",
        faint: false,
        properties: &[
            PropertySpec {
                local: "Lens",
                label: "Lens",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SerialNumber",
                label: "Serial Number",
                kind: PropertyKind::Shown,
            },
        ],
    },
    SchemaSpec {
        namespace: "http://ns.adobe.com/tiff/1.0/",
        prefix: "tiff",
        name: "EXIF Schema for TIFF Properties",
        faint: false,
        properties: &[
            PropertySpec {
                local: "ImageWidth",
                label: "Image Width",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ImageLength",
                label: "Image Length",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Compression",
                label: "Compression",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "PhotometricInterpretation",
                label: "Photometric Interpretation",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Orientation",
                label: "Orientation",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "SamplesPerPixel",
                label: "Samples Per Pixel",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "PlanarConfiguration",
                label: "Planar Configuration",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "YCbCrPositioning",
                label: "YCbCr Positioning",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "XResolution",
                label: "X Resolution",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "YResolution",
                label: "Y Resolution",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ResolutionUnit",
                label: "Resolution Unit",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "DateTime",
                label: "Date Time",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "ImageDescription",
                label: "Image Description",
                kind: PropertyKind::LanguageAlternative,
            },
            PropertySpec {
                local: "Make",
                label: "Make",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Model",
                label: "Model",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Software",
                label: "Software",
                kind: PropertyKind::Shown,
            },
            PropertySpec {
                local: "Artist",
                label: "Artist",
                kind: PropertyKind::Text,
            },
            PropertySpec {
                local: "Copyright",
                label: "Copyright",
                kind: PropertyKind::LanguageAlternative,
            },
        ],
    },
];
