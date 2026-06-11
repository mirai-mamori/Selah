export type MetaPair = [string, string];

export interface LunaAttachment {
  name?: string;
  file_name?: string;
  url?: string;
  link_type?: string;
  object_name?: string;
  download_action?: string;
  download_params?: MetaPair[];
}

export interface LunaDetailPage {
  title: string;
  course_name: string;
  sections: Array<{ heading: string; body: string }>;
  attachments: LunaAttachment[];
  meta: MetaPair[];
}

export interface LunaMaterialFile {
  display_name: string;
  file_name: string;
  object_name: string;
  resource_id: string;
  material_id: string;
  file_type: string;
  end_date: string;
  scan_status: string;
  link_type?: string;
  external_url?: string;
}

export interface LunaContentItem {
  title: string;
  url: string;
  period: string;
  status: string;
  item_type: string;
  description?: string;
  files?: LunaMaterialFile[];
}

export interface LunaCourseAnnouncement {
  title: string;
  info_id: string;
  start_date: string;
  end_date: string;
  is_new: boolean;
}

export interface LunaAttendanceItem {
  title: string;
  date: string;
  status: string;
  can_register?: boolean;
  idnumber?: string;
  attendance_id?: string;
  log_type?: string;
}

export interface LunaCourseContents {
  course_name: string;
  semester: string;
  teachers: string;
  ta_info: string;
  la_info: string;
  syllabus_url: string;
  grade_url: string;
  announcements: LunaCourseAnnouncement[];
  online_tools: Array<{ name: string; url: string; icon: string }>;
  materials: LunaContentItem[];
  reports: LunaContentItem[];
  examinations: LunaContentItem[];
  discussions: LunaContentItem[];
  surveys: LunaContentItem[];
  attendances: LunaAttendanceItem[];
}

export interface LunaSurveyQuestion {
  number: string;
  body: string;
  required: boolean;
  answer_type: string;
  answer_name?: string;
  options: Array<{ value: string; label: string }>;
}

export interface LunaSurveyDetail {
  title: string;
  description: string;
  period: string;
  anonymity: string;
  allow_edit: string;
  answer_status: string;
  respondent: string;
  attachments: Array<{
    file_name: string;
    object_name: string;
    url?: string;
    download_action?: string;
    download_params?: MetaPair[];
  }>;
  questions: LunaSurveyQuestion[];
  form_fields: MetaPair[];
  form_action: string;
}

export interface LunaDiscussionPost {
  title: string;
  author: string;
  date: string;
  content: string;
  status: string;
  thread_id: string;
  attachments?: LunaAttachment[];
}

export interface LunaDiscussionThread {
  title: string;
  course_name: string;
  description: string;
  meta: MetaPair[];
  posts: LunaDiscussionPost[];
}

export interface LunaInquiryPost {
  post_id: string;
  author: string;
  date: string;
  content_html: string;
  content_text: string;
  is_self: boolean;
  is_teacher: boolean;
  attachments: LunaAttachment[];
}

export interface LunaInquiryDetail {
  title: string;
  course_name: string;
  posts: LunaInquiryPost[];
  post_action: string;
  post_form_fields: MetaPair[];
  upload_action: string;
  upload_form_fields: MetaPair[];
  idnumber: string;
  inquiry_id: string;
}

export interface CourseDetail {
  fields: MetaPair[];
}

export interface KwicNotificationDetail {
  title: string;
  date: string;
  sender: string;
  body_html: string;
  attachments: Array<{ name: string; url: string }>;
}

export interface KwicCabinetItem {
  cabinet_id: string;
  list_id: string;
  name: string;
  level: number;
  updated_at: string;
  is_new: boolean;
  url: string;
}

export interface KwicCabinetReference {
  title: string;
  items: KwicCabinetItem[];
  raw_html_debug?: string | null;
}

export interface ControlEvent {
  owner?: string;
  target?: string;
  tabId?: string;
  action: string;
  payload?: unknown;
}

export interface DownloadMark {
  path?: string;
  loading?: boolean;
}
