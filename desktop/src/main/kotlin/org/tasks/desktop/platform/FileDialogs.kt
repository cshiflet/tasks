package org.tasks.desktop.platform

import java.awt.FileDialog
import java.awt.Frame
import java.io.File
import javax.swing.JFileChooser
import javax.swing.filechooser.FileNameExtensionFilter

object FileDialogs {

    /**
     * Show a file open dialog and return the selected file.
     */
    fun showOpenDialog(
        title: String = "Open File",
        initialDirectory: File? = null,
        filters: List<FileFilter> = emptyList(),
    ): File? {
        return try {
            // Try native AWT dialog first
            showAwtOpenDialog(title, initialDirectory, filters)
        } catch (e: Exception) {
            // Fall back to Swing dialog
            showSwingOpenDialog(title, initialDirectory, filters)
        }
    }

    /**
     * Show a file save dialog and return the selected file.
     */
    fun showSaveDialog(
        title: String = "Save File",
        initialDirectory: File? = null,
        suggestedName: String? = null,
        filters: List<FileFilter> = emptyList(),
    ): File? {
        return try {
            showAwtSaveDialog(title, initialDirectory, suggestedName)
        } catch (e: Exception) {
            showSwingSaveDialog(title, initialDirectory, suggestedName, filters)
        }
    }

    /**
     * Show a directory chooser dialog.
     */
    fun showDirectoryDialog(
        title: String = "Select Directory",
        initialDirectory: File? = null,
    ): File? {
        val chooser = JFileChooser().apply {
            dialogTitle = title
            fileSelectionMode = JFileChooser.DIRECTORIES_ONLY
            initialDirectory?.let { currentDirectory = it }
        }

        return if (chooser.showOpenDialog(null) == JFileChooser.APPROVE_OPTION) {
            chooser.selectedFile
        } else null
    }

    /**
     * Show a multi-file open dialog.
     */
    fun showMultiOpenDialog(
        title: String = "Open Files",
        initialDirectory: File? = null,
        filters: List<FileFilter> = emptyList(),
    ): List<File> {
        val chooser = JFileChooser().apply {
            dialogTitle = title
            isMultiSelectionEnabled = true
            fileSelectionMode = JFileChooser.FILES_ONLY
            initialDirectory?.let { currentDirectory = it }
            filters.forEach { filter ->
                addChoosableFileFilter(
                    FileNameExtensionFilter(filter.description, *filter.extensions.toTypedArray())
                )
            }
        }

        return if (chooser.showOpenDialog(null) == JFileChooser.APPROVE_OPTION) {
            chooser.selectedFiles.toList()
        } else emptyList()
    }

    private fun showAwtOpenDialog(
        title: String,
        initialDirectory: File?,
        filters: List<FileFilter>,
    ): File? {
        val dialog = FileDialog(null as Frame?, title, FileDialog.LOAD).apply {
            initialDirectory?.let { directory = it.absolutePath }
            if (filters.isNotEmpty()) {
                setFilenameFilter { _, name ->
                    filters.any { filter ->
                        filter.extensions.any { ext ->
                            name.endsWith(".$ext", ignoreCase = true)
                        }
                    }
                }
            }
            isVisible = true
        }

        return if (dialog.file != null) {
            File(dialog.directory, dialog.file)
        } else null
    }

    private fun showAwtSaveDialog(
        title: String,
        initialDirectory: File?,
        suggestedName: String?,
    ): File? {
        val dialog = FileDialog(null as Frame?, title, FileDialog.SAVE).apply {
            initialDirectory?.let { directory = it.absolutePath }
            suggestedName?.let { file = it }
            isVisible = true
        }

        return if (dialog.file != null) {
            File(dialog.directory, dialog.file)
        } else null
    }

    private fun showSwingOpenDialog(
        title: String,
        initialDirectory: File?,
        filters: List<FileFilter>,
    ): File? {
        val chooser = JFileChooser().apply {
            dialogTitle = title
            fileSelectionMode = JFileChooser.FILES_ONLY
            initialDirectory?.let { currentDirectory = it }
            filters.forEach { filter ->
                addChoosableFileFilter(
                    FileNameExtensionFilter(filter.description, *filter.extensions.toTypedArray())
                )
            }
        }

        return if (chooser.showOpenDialog(null) == JFileChooser.APPROVE_OPTION) {
            chooser.selectedFile
        } else null
    }

    private fun showSwingSaveDialog(
        title: String,
        initialDirectory: File?,
        suggestedName: String?,
        filters: List<FileFilter>,
    ): File? {
        val chooser = JFileChooser().apply {
            dialogTitle = title
            fileSelectionMode = JFileChooser.FILES_ONLY
            initialDirectory?.let { currentDirectory = it }
            suggestedName?.let { selectedFile = File(it) }
            filters.forEach { filter ->
                addChoosableFileFilter(
                    FileNameExtensionFilter(filter.description, *filter.extensions.toTypedArray())
                )
            }
        }

        return if (chooser.showSaveDialog(null) == JFileChooser.APPROVE_OPTION) {
            chooser.selectedFile
        } else null
    }

    data class FileFilter(
        val description: String,
        val extensions: List<String>,
    ) {
        companion object {
            val IMAGES = FileFilter("Images", listOf("png", "jpg", "jpeg", "gif", "bmp", "webp"))
            val DOCUMENTS = FileFilter("Documents", listOf("pdf", "doc", "docx", "txt", "md"))
            val ALL = FileFilter("All Files", listOf("*"))
        }
    }
}
