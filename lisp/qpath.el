;;; qpath.el --- Emacs integration for qpath -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Akinori Musha

;; Author: Akinori Musha <knu@idaemons.org>
;; Maintainer: Akinori Musha <knu@idaemons.org>
;; Version: 0.1.0
;; Package-Requires: ((emacs "29.1") (transient "0.4.3"))
;; Keywords: convenience, files, tools
;; URL: https://github.com/knu/qpath
;; SPDX-License-Identifier: MIT

;;; Commentary:

;; qpath.el provides transient menus backed by the qpath command line tool.
;; Use `qpath-visit-file' to visit a registered file, and
;; `qpath-insert-directory' to insert a registered directory path at point.
;;
;; The cache is refreshed on demand.  Call `qpath-start-auto-update' from your
;; init file when periodic background refreshes are desired.

;;; Code:

(require 'cl-lib)
(require 'json)
(require 'transient)

(defgroup qpath nil
  "Visit or insert paths registered with qpath."
  :group 'tools
  :prefix "qpath-")

(defcustom qpath-command "qpath"
  "Command name or file name for qpath."
  :type 'string
  :group 'qpath)

(defcustom qpath-update-interval 300
  "Seconds between automatic qpath cache refreshes."
  :type 'number
  :group 'qpath)

(defcustom qpath-after-visit-file-functions nil
  "Hook run after `qpath-visit-file' visits a file.
Each function is called with the visited file path."
  :type 'hook
  :group 'qpath)

(defcustom qpath-visit-file-extra-sections nil
  "Additional transient sections appended to `qpath-visit-file'.
Each element is a transient group vector."
  :type '(repeat sexp)
  :group 'qpath)

(defcustom qpath-insert-directory-extra-sections nil
  "Additional transient sections appended to `qpath-insert-directory'.
Each element is a transient group vector."
  :type '(repeat sexp)
  :group 'qpath)

(defvar qpath--file-cache nil
  "Cached qpath file entries.")

(defvar qpath--directory-cache nil
  "Cached qpath directory entries.")

(defvar qpath--update-timer nil
  "Timer used to refresh qpath caches.")

(defun qpath--read (type)
  "Return registered qpath entries of TYPE."
  (unless (executable-find qpath-command)
    (error "%s is not found" qpath-command))
  (with-temp-buffer
    (unless (zerop (call-process qpath-command nil t nil
                                 "ls" "--type" type "--format" "json"))
      (error "%s ls --type %s failed" qpath-command type))
    (json-parse-string (buffer-string)
                       :array-type 'list
                       :object-type 'hash-table)))

(defun qpath--entry (entry key)
  "Return KEY from qpath ENTRY."
  (gethash key entry))

(defun qpath--file-suffixes ()
  "Return transient suffixes for registered files."
  (cl-loop for entry in qpath--file-cache
           for key = (qpath--entry entry "abbr")
           for desc = (qpath--entry entry "desc")
           for path = (qpath--entry entry "path")
           when (and key desc path)
           collect `(,key
                     ,desc
                     (lambda ()
                       (interactive)
                       (find-file ,path)
                       (run-hook-with-args
                        'qpath-after-visit-file-functions ,path))
                     :if (lambda () (file-exists-p ,path)))))

(defun qpath--directory-suffixes ()
  "Return transient suffixes for registered directories."
  (cl-loop for entry in qpath--directory-cache
           for key = (qpath--entry entry "abbr")
           for desc = (qpath--entry entry "desc")
           for path = (qpath--entry entry "path")
           for shell-path = (qpath--entry entry "shell_path")
           when (and key desc path shell-path)
           collect `(,key
                     ,desc
                     (lambda ()
                       (interactive)
                       (insert ,shell-path))
                     :if (lambda () (file-directory-p ,path)))))

(defun qpath--define-transients ()
  "Define transient commands from the qpath cache."
  (eval
   `(transient-define-prefix qpath--visit-file-transient ()
      "Visit a registered file."
      ,@(append (list `["Visit" ,@(qpath--file-suffixes)])
                qpath-visit-file-extra-sections)))
  (eval
   `(transient-define-prefix qpath--insert-directory-transient ()
      "Insert a registered directory."
      ,@(append (list `["Insert" ,@(qpath--directory-suffixes)])
                qpath-insert-directory-extra-sections))))

(defun qpath-update (&optional quiet)
  "Refresh qpath caches.
With QUIET, do not report refresh failures."
  (interactive)
  (condition-case err
      (let ((files (qpath--read "file"))
            (directories (qpath--read "directory")))
        (setq qpath--file-cache files)
        (setq qpath--directory-cache directories)
        (qpath--define-transients)
        (unless quiet
          (message "Updated qpath cache"))
        t)
    (error
     (unless quiet
       (message "Failed to update qpath cache: %s" (error-message-string err)))
     nil)))

(defun qpath--ensure-cache ()
  "Refresh the qpath cache when it is empty."
  (unless (or qpath--file-cache qpath--directory-cache)
    (qpath-update)))

;;;###autoload
(defun qpath-visit-file ()
  "Visit a registered file."
  (interactive)
  (qpath--ensure-cache)
  (transient-setup 'qpath--visit-file-transient))

;;;###autoload
(defun qpath-insert-directory ()
  "Insert a registered directory."
  (interactive)
  (qpath--ensure-cache)
  (transient-setup 'qpath--insert-directory-transient))

;;;###autoload
(defun qpath-start-auto-update (&optional quiet)
  "Refresh qpath caches now and periodically.
With QUIET, do not report refresh failures."
  (interactive)
  (qpath-stop-auto-update)
  (qpath-update quiet)
  (setq qpath--update-timer
        (run-with-timer qpath-update-interval
                        qpath-update-interval
                        #'qpath-update
                        t)))

;;;###autoload
(defun qpath-stop-auto-update ()
  "Stop refreshing qpath caches periodically."
  (interactive)
  (when (timerp qpath--update-timer)
    (cancel-timer qpath--update-timer))
  (setq qpath--update-timer nil))

(qpath--define-transients)

(provide 'qpath)

;;; qpath.el ends here
